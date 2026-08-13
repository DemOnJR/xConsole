//! Goal-driven autonomous mode (/goal): a persistent controller that runs
//! plan → act → verify cycles against a GoalSpec until the goal is met, waits
//! when external-world timing is involved (e.g. Google reindexing), and records
//! everything on a live kanban board.
//!
//! The loop reuses `agent::run_turn` (same as cron's "prompt" jobs) and the
//! existing `safety::ApprovalRegistry` gate. Tool calls like `goal_add_task`
//! mutate the session via the DB and emit `goal://` events for the kanban node.

use std::sync::Arc;

use chrono::Utc;
use dashmap::DashSet;
use tauri::{AppHandle, Emitter};

use crate::ai::provider::{ChatMessage, StreamEvent};
use crate::ai::safety::{self, ApprovalRegistry};
use crate::ai::tools::ToolContext;
use crate::ai::{agent, AgentHome};
use crate::ssh::SessionManager;
use crate::storage::models::{GoalSession, GoalSpec, GoalTask};
use crate::storage::Db;

/// Goal ids currently running (prevents overlapping loops). Shared app state.
#[derive(Clone, Default)]
pub struct GoalRunning {
    pub goals: Arc<DashSet<String>>,
}

/// Shared handles the goal loop needs. Mirrors `CronContext`.
#[derive(Clone)]
pub struct GoalContext {
    pub app: AppHandle,
    pub db: Db,
    pub sessions: SessionManager,
    pub home: AgentHome,
    pub approvals: ApprovalRegistry,
    pub running: GoalRunning,
}

/// Event channel for one goal session's live updates.
pub fn goal_event(goal_id: &str) -> String {
    format!("goal://{goal_id}")
}

/// Parse the goal id out of a ToolContext session id ("goal:<id>").
pub fn goal_id_from_session(session_id: &str) -> Option<String> {
    session_id.strip_prefix("goal:").map(|s| s.to_string())
}

/// Load a goal session by id from the DB.
pub fn load_goal(db: &Db, id: &str) -> Result<Option<GoalSession>, String> {
    db.get_goal(id).map_err(|e| e.to_string())
}

/// Persist a goal session and emit a status event to the kanban node.
pub fn save_goal(ctx: &GoalContext, goal: &GoalSession) -> Result<(), String> {
    ctx.db.update_goal(goal).map_err(|e| e.to_string())?;
    let _ = ctx.app.emit(&goal_event(&goal.id), StreamEvent::Status(goal.status.clone()));
    Ok(())
}

/// Parse the GoalSpec from a session's spec_json.
pub fn parse_spec(goal: &GoalSession) -> Option<GoalSpec> {
    serde_json::from_str(&goal.spec_json).ok()
}

/// Parse the kanban cards from a session's kanban_json.
pub fn parse_kanban(goal: &GoalSession) -> Vec<GoalTask> {
    serde_json::from_str(&goal.kanban_json).unwrap_or_default()
}

/// Serialize kanban cards back into a session.
pub fn set_kanban(goal: &mut GoalSession, tasks: Vec<GoalTask>) {
    goal.kanban_json = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
}

/// Notify the user via an OS notification (best-effort).
fn notify_user(app: &AppHandle, title: &str, body: &str) {
    let _ = app.emit("goal://notify", serde_json::json!({ "title": title, "body": body }));
}

/// The default wait before re-checking when "too early to tell" with no learned
/// latency data — conservative ~3 days for a small site.
const DEFAULT_REINDEX_WAIT_SECS: i64 = 3 * 24 * 3600;

/// Run one plan → act → verify cycle for an active goal. Returns the new status
/// ("active" to continue, "waiting" to sleep, "done"/"blocked"/"stopped" to exit).
async fn run_cycle(ctx: &GoalContext, goal: &GoalSession) -> Result<String, String> {
    let event = goal_event(&goal.id);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let app = ctx.app.clone();
    let event_owned = event.clone();
    let forward = tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app.emit(&event_owned, ev);
        }
    });

    // Build a ToolContext for this cycle, exactly like cron's run_prompt_job.
    let hooks_cfg = if ctx
        .db
        .get_setting("agent.hooks_enabled")
        .ok()
        .flatten()
        .as_deref()
        == Some("false")
    {
        crate::ai::hooks::HooksConfig::default()
    } else {
        crate::ai::hooks::HooksConfig::load(&ctx.home)
    };

    // The cycle prompt: the loop tells the agent to plan (kanban), act, and verify.
    let spec = parse_spec(goal).unwrap_or(GoalSpec {
        objective: goal.raw_request.clone(),
        success_criteria: vec![],
        check_method: String::new(),
        check_tooling: vec![],
        hard_constraints: vec![],
        max_cycles: None,
    });
    let kanban = parse_kanban(goal);
    let kanban_summary: Vec<String> = kanban
        .iter()
        .map(|t| format!("[{}] {} — {}", t.column, t.title, t.result.clone().unwrap_or_default()))
        .collect();
    let prompt = format!(
        "You are driving an autonomous goal. Objective: {objective}\n\
         Success criteria (you may ONLY conclude 'done' via goal_check_criteria with evidence):\n\
         {criteria}\n\
         Check method: {check}\n\
         Hard constraints (never violate): {constraints}\n\
         Current kanban:\n{kanban}\n\n\
         This cycle: plan the next concrete step (use goal_add_task / goal_update_task), \
         execute it with your normal tools, then call goal_check_criteria. If a change needs \
         external time (e.g. search reindexing), call goal_schedule_wait with a sensible delay.",
        objective = spec.objective,
        criteria = spec.success_criteria.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n"),
        check = spec.check_method,
        constraints = if spec.hard_constraints.is_empty() {
            "(none)".to_string()
        } else {
            spec.hard_constraints.join("; ")
        },
        kanban = if kanban_summary.is_empty() {
            "(empty)".to_string()
        } else {
            kanban_summary.join("\n")
        },
    );

    let tc = ToolContext {
        app: ctx.app.clone(),
        db: ctx.db.clone(),
        sessions: ctx.sessions.clone(),
        home: ctx.home.clone(),
        approvals: ctx.approvals.clone(),
        prompts: crate::ai::interaction::PromptRegistry::new(),
        session_state: crate::ai::interaction::SessionState::new(),
        session_id: format!("goal:{}", goal.id),
        targets: Vec::new(),
        safety: safety::global_safety_mode(&ctx.db),
        plan_mode: false,
        workspace_id: None,
        canvas: Vec::new(),
        edits: crate::ai::edits::EditJournal::with_db(ctx.db.clone()),
        hooks: hooks_cfg,
    };

    let messages = vec![ChatMessage::user(prompt)];
    let result = agent::run_turn(&tc, None, messages, false, &tx).await;
    drop(tx);
    let _ = forward.await;

    // Re-read the goal: the agent's goal_* tool calls updated it during the turn.
    let fresh = ctx.db.get_goal(&goal.id).map_err(|e| e.to_string())?;
    let fresh = fresh.ok_or_else(|| "goal session disappeared".to_string())?;

    if result.is_err() {
        return Ok("blocked".to_string()); // error → surface as blocked, not done
    }

    Ok(fresh.status.clone())
}

/// Run the full loop for one goal until it reaches a terminal state.
pub async fn run_loop(ctx: &GoalContext, goal_id: &str) {
    if !ctx.running.goals.insert(goal_id.to_string()) {
        return; // already running
    }
    // Drop guard on exit.
    struct Guard(GoalRunning, String);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.goals.remove(&self.1);
        }
    }
    let _guard = Guard(ctx.running.clone(), goal_id.to_string());

    let Some(mut goal) = load_goal(&ctx.db, goal_id).ok().flatten() else {
        return;
    };

    loop {
        if goal.status != "active" {
            // waiting/blocked/done/stopped → stop driving (the tick resumes waiting).
            if goal.status == "waiting" {
                if let Some(at) = goal.next_check_at.clone() {
                    let _ = ctx.app.emit(
                        &goal_event(&goal.id),
                        StreamEvent::Status(format!("waiting until {at}")),
                    );
                }
            }
            return;
        }

        let status = match run_cycle(ctx, &goal).await {
            Ok(s) => s,
            Err(e) => {
                let _ = ctx
                    .app
                    .emit(&goal_event(&goal.id), StreamEvent::Error(e.clone()));
                return;
            }
        };

        // Re-load after the cycle (the agent may have moved to waiting/done itself).
        let Some(fresh) = load_goal(&ctx.db, goal_id).ok().flatten() else {
            return;
        };
        goal = fresh;
        if goal.status != "active" {
            continue; // let the loop exit / handle waiting above
        }

        // The agent stayed "active" without resolving: count a cycle and check the cap.
        let cycles = goal.cycles + 1;
        goal.cycles = cycles;
        let spec = parse_spec(&goal);
        if let Some(max) = spec.as_ref().and_then(|s| s.max_cycles) {
            if cycles >= max {
                goal.status = "blocked".to_string();
                goal.finished_at = Some(Utc::now().to_rfc3339());
                let _ = save_goal(ctx, &goal);
                notify_user(&ctx.app, "Goal blocked", &format!("{} reached max cycles", goal.title));
                return;
            }
        }
        let _ = save_goal(ctx, &goal);

        // If the agent called goal_schedule_wait, the status is now "waiting".
        if goal.status == "waiting" {
            continue;
        }
        // Safety valve: if the cycle made no status change and the spec had no
        // explicit wait, force a short wait to avoid hot-looping.
        if status == "active" && goal.status == "active" {
            goal.status = "waiting".to_string();
            goal.next_check_at = Some(
                (Utc::now() + chrono::Duration::seconds(DEFAULT_REINDEX_WAIT_SECS)).to_rfc3339(),
            );
            let _ = save_goal(ctx, &goal);
        }
    }
}

/// Called from the cron tick (or a sibling): resume any waiting goals that are due.
pub async fn resume_due_goals(ctx: &GoalContext) {
    let Ok(goals) = ctx.db.list_due_goals() else {
        return;
    };
    for g in goals {
        // Flip back to active so run_loop drives it.
        let mut g = g;
        g.status = "active".to_string();
        g.next_check_at = None;
        if ctx.db.update_goal(&g).is_ok() {
            let ctx = ctx.clone();
            let id = g.id.clone();
            tauri::async_runtime::spawn(async move {
                run_loop(&ctx, &id).await;
            });
        }
    }
}

/// Spawn the goal-resume ticker on the Tauri async runtime (same 30s cadence as
/// cron's scheduler; resumes due "waiting" goals after an app restart too).
pub fn spawn_tick(ctx: GoalContext) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            resume_due_goals(&ctx).await;
        }
    });
}
