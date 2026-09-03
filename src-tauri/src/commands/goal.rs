//! Tauri commands for /goal (autonomous goal sessions). Mirrors the conventions
//! in commands/ai.rs and commands/session.rs.

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::ai::goal::{run_loop, GoalContext, GoalRunning};
use crate::ai::interaction::SessionState;
use crate::ai::safety::ApprovalRegistry;
use crate::ai::AgentHome;
use crate::ssh::SessionManager;
use crate::storage::models::GoalSession;
use crate::storage::Db;

/// Create a goal session in "intake" status. The intake Q&A turn runs in the
/// normal chat; `confirm_goal` flips it to "active" and starts the loop.
#[tauri::command]
pub async fn start_goal(
    app: AppHandle,
    db: State<'_, Db>,
    text: String,
    // Which project this goal is about. The canvas passes the active workspace, so a
    // goal started from a project stays filed under it instead of into a global pool.
    workspace_id: Option<String>,
) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    // Title = first line / first 48 chars of the request.
    let title = text
        .lines()
        .next()
        .unwrap_or(&text)
        .trim()
        .chars()
        .take(48)
        .collect::<String>();
    let goal = GoalSession {
        id: id.clone(),
        title,
        raw_request: text,
        spec_json: "{}".to_string(),
        status: "intake".to_string(),
        kanban_json: "[]".to_string(),
        memory_json: "{}".to_string(),
        next_check_at: None,
        cycles: 0,
        created_at: None,
        updated_at: None,
        finished_at: None,
        // A goal the user starts by hand runs as the default agent; personas are
        // attached by `agent_delegate`.
        persona_id: None,
        workspace_id: workspace_id.filter(|s| !s.is_empty()),
        // Written when it finishes, by the agent that finishes it.
        outcome: None,
        request_id: None,
        reported_at: None,
        pr_number: None,
        approval_state: None,
    };
    db.insert_goal(&goal).map_err(|e| e.to_string())?;
    let _ = app.emit(&crate::ai::goal::goal_event(&id), crate::ai::provider::StreamEvent::Status("intake".into()));
    Ok(id)
}

/// Flip a goal from "intake" to "active" and start the loop.
#[tauri::command]
pub async fn confirm_goal(
    app: AppHandle,
    db: State<'_, Db>,
    home: State<'_, AgentHome>,
    sessions: State<'_, SessionManager>,
    approvals: State<'_, ApprovalRegistry>,
    running: State<'_, GoalRunning>,
    session_state: State<'_, SessionState>,
    id: String,
    targets: Option<Vec<String>>,
) -> Result<(), String> {
    let mut goal = db
        .get_goal(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())?;
    if goal.status != "intake" {
        return Err(format!("goal is in '{}' status, not intake", goal.status));
    }
    if let Some(ids) = targets.filter(|t| !t.is_empty()) {
        let mut spec = crate::ai::goal::parse_spec(&goal).unwrap_or(crate::storage::models::GoalSpec {
            objective: goal.raw_request.clone(),
            success_criteria: vec![],
            check_method: String::new(),
            check_tooling: vec![],
            hard_constraints: vec![],
            max_cycles: None,
            vps_targets: vec![],
        });
        spec.vps_targets = ids;
        if let Ok(json) = serde_json::to_string(&spec) {
            goal.spec_json = json;
        }
    }
    goal.status = "active".to_string();
    goal.next_check_at = None;
    db.update_goal(&goal).map_err(|e| e.to_string())?;

    spawn_loop(
        app.clone(),
        db.inner().clone(),
        sessions.inner().clone(),
        home.inner().clone(),
        approvals.inner().clone(),
        running.inner().clone(),
        session_state.inner().clone(),
        id.clone(),
    );
    let _ = app.emit(
        &crate::ai::goal::goal_event(&id),
        crate::ai::provider::StreamEvent::Status("active".into()),
    );
    Ok(())
}

fn spawn_loop(
    app: AppHandle,
    db: crate::storage::Db,
    sessions: SessionManager,
    home: AgentHome,
    approvals: ApprovalRegistry,
    running: GoalRunning,
    session_state: SessionState,
    id: String,
) {
    let ctx = GoalContext {
        app,
        db,
        sessions,
        home,
        approvals,
        running,
        session_state,
    };
    tauri::async_runtime::spawn(async move {
        run_loop(&ctx, &id).await;
    });
}

/// Pause a running goal, without the Tauri wrapper.
///
/// Split out because the desktop had three buttons — pause, continue, stop — and the
/// agents had none of them. An orchestrator could watch one of its own agents work its
/// way into the wrong thing and had no way to intervene except to tell the user to open
/// the app. These three functions are what `task_pause` / `task_resume` / `task_stop`
/// call, so a phone and the desktop stop a task by exactly the same route.
pub fn pause_goal_inner(
    app: &AppHandle,
    db: &Db,
    session_state: &SessionState,
    id: &str,
) -> Result<GoalSession, String> {
    let mut goal = db
        .get_goal(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())?;
    if goal.status == "stopped" || goal.status == "done" {
        return Err(format!("goal is already {}", goal.status));
    }
    goal.status = "paused".to_string();
    goal.next_check_at = None;
    db.update_goal(&goal).map_err(|e| e.to_string())?;
    // Cancels the turn in flight as well as the next one: a paused task that keeps
    // running until its current tool call returns is not paused.
    session_state.cancel(&format!("goal:{id}"));
    let _ = app.emit(
        &crate::ai::goal::goal_event(id),
        crate::ai::provider::StreamEvent::Status("paused".into()),
    );
    Ok(goal)
}

/// Resume a paused / waiting / blocked goal, without the Tauri wrapper.
pub fn resume_goal_inner(
    app: &AppHandle,
    db: &Db,
    session_state: &SessionState,
    id: &str,
) -> Result<GoalSession, String> {
    let mut goal = db
        .get_goal(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())?;
    if goal.status == "stopped" || goal.status == "done" || goal.status == "intake" {
        return Err(format!("cannot continue a goal in '{}' status", goal.status));
    }
    goal.status = "active".to_string();
    goal.next_check_at = None;
    goal.finished_at = None;
    db.update_goal(&goal).map_err(|e| e.to_string())?;
    session_state.clear_cancel(&format!("goal:{id}"));
    crate::ai::goal::spawn_from_app(app, id);
    let _ = app.emit(
        &crate::ai::goal::goal_event(id),
        crate::ai::provider::StreamEvent::Status("active".into()),
    );
    Ok(goal)
}

/// Terminate a goal, without the Tauri wrapper. Idempotent.
pub fn stop_goal_inner(
    app: &AppHandle,
    db: &Db,
    session_state: &SessionState,
    id: &str,
) -> Result<GoalSession, String> {
    let mut goal = db
        .get_goal(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())?;
    goal.status = "stopped".to_string();
    goal.finished_at = Some(chrono::Utc::now().to_rfc3339());
    db.update_goal(&goal).map_err(|e| e.to_string())?;
    session_state.cancel(&format!("goal:{id}"));
    let _ = app.emit(
        &crate::ai::goal::goal_event(id),
        crate::ai::provider::StreamEvent::Status("stopped".into()),
    );
    Ok(goal)
}

/// Pause a running goal. It will not resume until the user presses Continue.
#[tauri::command]
pub async fn pause_goal(
    app: AppHandle,
    db: State<'_, Db>,
    session_state: State<'_, SessionState>,
    id: String,
) -> Result<(), String> {
    pause_goal_inner(&app, &db, &session_state, &id).map(|_| ())
}

/// Resume a paused / waiting / blocked goal.
#[tauri::command]
pub async fn continue_goal(
    app: AppHandle,
    db: State<'_, Db>,
    home: State<'_, AgentHome>,
    sessions: State<'_, SessionManager>,
    approvals: State<'_, ApprovalRegistry>,
    running: State<'_, GoalRunning>,
    session_state: State<'_, SessionState>,
    id: String,
) -> Result<(), String> {
    // `home` / `sessions` / `approvals` / `running` stay in the signature: the frontend
    // passes them and `spawn_from_app` reads the same handles back out of the app.
    let _ = (&home, &sessions, &approvals, &running);
    resume_goal_inner(&app, &db, &session_state, &id).map(|_| ())
}

/// Terminate a goal (status → "stopped", finished_at set). Idempotent.
#[tauri::command]
pub async fn stop_goal(
    app: AppHandle,
    db: State<'_, Db>,
    session_state: State<'_, SessionState>,
    id: String,
) -> Result<(), String> {
    stop_goal_inner(&app, &db, &session_state, &id).map(|_| ())
}

#[tauri::command]
pub async fn get_goal(db: State<'_, Db>, id: String) -> Result<GoalSession, String> {
    db.get_goal(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())
}

#[tauri::command]
pub async fn list_goals(db: State<'_, Db>) -> Result<Vec<GoalSession>, String> {
    db.list_goals().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_goal(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_goal(&id).map_err(|e| e.to_string())
}
