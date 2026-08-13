//! Tauri commands for /goal (autonomous goal sessions). Mirrors the conventions
//! in commands/ai.rs and commands/session.rs.

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::ai::goal::{run_loop, GoalContext, GoalRunning};
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
    id: String,
) -> Result<(), String> {
    let mut goal = db
        .get_goal(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())?;
    if goal.status != "intake" {
        return Err(format!("goal is in '{}' status, not intake", goal.status));
    }
    goal.status = "active".to_string();
    goal.next_check_at = None;
    db.update_goal(&goal).map_err(|e| e.to_string())?;

    let ctx = GoalContext {
        app: app.clone(),
        db: db.inner().clone(),
        sessions: sessions.inner().clone(),
        home: home.inner().clone(),
        approvals: approvals.inner().clone(),
        running: running.inner().clone(),
    };
    let loop_id = id.clone();
    tauri::async_runtime::spawn(async move {
        run_loop(&ctx, &loop_id).await;
    });
    let _ = app.emit(
        &crate::ai::goal::goal_event(&id),
        crate::ai::provider::StreamEvent::Status("active".into()),
    );
    Ok(())
}

/// Stop a goal (status → "stopped", finished_at set). Idempotent.
#[tauri::command]
pub async fn stop_goal(
    app: AppHandle,
    db: State<'_, Db>,
    id: String,
) -> Result<(), String> {
    let mut goal = db
        .get_goal(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal not found".to_string())?;
    goal.status = "stopped".to_string();
    goal.finished_at = Some(chrono::Utc::now().to_rfc3339());
    db.update_goal(&goal).map_err(|e| e.to_string())?;
    let _ = app.emit(
        &crate::ai::goal::goal_event(&id),
        crate::ai::provider::StreamEvent::Status("stopped".into()),
    );
    Ok(())
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
