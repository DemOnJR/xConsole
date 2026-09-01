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
use crate::ai::interaction::SessionState;
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
    pub session_state: SessionState,
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

/// Brief pause between cycles so we do not hammer the provider. Not a user wait.
const CYCLE_GAP: std::time::Duration = std::time::Duration::from_secs(2);

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
        vps_targets: vec![],
    });
    // The persona this goal belongs to, if any. Everything below — prompt, servers,
    // trust level, model — reads from it, so a delegated run genuinely behaves like
    // the named agent rather than merely being labelled with its name.
    let persona = goal
        .persona_id
        .as_deref()
        .and_then(|id| crate::ai::persona::resolve(&ctx.db, id));

    let kanban = parse_kanban(goal);
    let kanban_summary: Vec<String> = {
        let roots: Vec<&GoalTask> = kanban
            .iter()
            .filter(|t| {
                t.parent_id
                    .as_ref()
                    .map(|p| !kanban.iter().any(|o| o.id == *p))
                    .unwrap_or(true)
            })
            .collect();
        let mut lines = Vec::new();
        fn walk(tasks: &[GoalTask], t: &GoalTask, indent: &str, lines: &mut Vec<String>) {
            let extra = t.result.clone().unwrap_or_default();
            lines.push(format!(
                "{indent}[{}] {} — {extra}",
                t.column,
                t.title
            ));
            for child in tasks.iter().filter(|c| c.parent_id.as_deref() == Some(&t.id)) {
                walk(tasks, child, &format!("{indent}  "), lines);
            }
        }
        for t in roots {
            walk(&kanban, t, "", &mut lines);
        }
        lines
    };

    // Deterministic state & criteria sorting (Rick-style EpochHash)
    let mut sorted_criteria = spec.success_criteria.clone();
    sorted_criteria.sort();
    let mut sorted_constraints = spec.hard_constraints.clone();
    sorted_constraints.sort();
    let mut sorted_targets = spec.vps_targets.clone();
    sorted_targets.sort();

    let raw_epoch = format!(
        "{}:{}:{}:{}",
        spec.objective.trim(),
        sorted_criteria.join("|"),
        sorted_constraints.join("|"),
        sorted_targets.join("|")
    );
    let epoch_hash = format!("{:08x}", raw_epoch.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32)));

    // Identity, then the chain of command, then anything colleagues have said since
    // the last cycle. Delivered as part of the prompt rather than left for the agent
    // to fetch: a message nobody thought to check for is a message that never
    // arrived, and the whole point of the hierarchy is that agents hear each other.
    let persona_block = match persona.as_ref() {
        Some(p) => {
            let all = ctx.db.list_personas().unwrap_or_default();
            let mut block = crate::ai::persona::prompt_block(p);
            block.push_str(&crate::ai::persona::hierarchy_block(&all, p));
            if let Ok(unread) =
                ctx.db.unread_agent_messages(Some(&p.id))
            {
                if !unread.is_empty() {
                    block.push_str("\n\nNew messages for you:\n");
                    for m in &unread {
                        let from = m
                            .from_id
                            .as_deref()
                            .and_then(|id| all.iter().find(|c| c.id == id))
                            .map(|c| c.name.as_str())
                            .unwrap_or("the user");
                        block.push_str(&format!("- [{}] {from}: {}\n", m.kind, m.body));
                    }
                    // Marked read on delivery, so a message already folded into this
                    // prompt is not re-delivered and re-acted on every cycle.
                    let ids: Vec<String> = unread.iter().map(|m| m.id.clone()).collect();
                    if let Err(e) = ctx.db.mark_agent_messages_read(&ids) {
                        crate::diag(&format!("goal {}: could not mark messages read: {e}", goal.id));
                    }
                }
            }
            format!("{block}\n\n")
        }
        None => String::new(),
    };

    let prompt = format!(
        "{persona_block}\
         You are driving an autonomous goal (Epoch: {epoch_hash}). Keep the kanban LIVE this cycle.\n\
         Objective: {objective}\n\
         Success criteria (you may ONLY conclude 'done' via goal_check_criteria with evidence):\n\
         {criteria}\n\
         Check method: {check}\n\
         Hard constraints (never violate): {constraints}\n\
         Selected VPS targets (use these exact vps_id values with run_command):\n{targets}\n\
         Current kanban:\n{kanban}\n\n\
         Rules:\n\
         - Before you work, goal_add_task (column in_progress) for the concrete step.\n\
         - If a step has more than one action, add sub-tasks with goal_add_task parent_id=<parent>.\n\
         - As you work, goal_update_task to move cards: in_progress → testing → done.\n\
         - After every action, goal_update_task with note= what you did (commands, findings, errors).\n\
         - Use waiting only when YOU are blocked on real external time the user asked for.\n\
         - Never invent 'blocked' cards that just say they depend on another card — do the work.\n\
         - Do NOT call goal_schedule_wait unless the user specified a delay/timeout.\n\
         - If nothing is waiting, keep going: next check, next card.\n\
         This cycle: pick the next unfinished card (or add one), execute it with tools, update the board, then goal_check_criteria (verdict not_yet unless truly done).",
        persona_block = persona_block,
        epoch_hash = epoch_hash,
        objective = spec.objective,
        criteria = sorted_criteria.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n"),
        check = spec.check_method,
        constraints = if sorted_constraints.is_empty() {
            "(none)".to_string()
        } else {
            sorted_constraints.join("; ")
        },
        targets = if sorted_targets.is_empty() {
            "(none selected — call list_vps_targets / ask, or use hosts the user named)".to_string()
        } else {
            sorted_targets.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n")
        },
        kanban = if kanban_summary.is_empty() {
            "(empty — add the first card now)".to_string()
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
        session_state: ctx.session_state.clone(),
        session_id: format!("goal:{}", goal.id),
        targets: crate::ai::persona::effective_targets(persona.as_ref(), &spec.vps_targets),
        // A persona is how the user says "this one may restart services unattended,
        // that one may only look" — which means nothing unless the loop honours it.
        safety: crate::ai::persona::safety_mode(&ctx.db, persona.as_ref()),
        plan_mode: false,
        // The project this task belongs to. Without it a delegated agent runs blind:
        // no project brief, no workspace memory, no CLAUDE.md — it is handed an
        // objective with no idea which codebase it is about, and everything it says
        // lands in one undifferentiated pool with every other project's chatter.
        workspace_id: goal.workspace_id.clone(),
        canvas: Vec::new(),
        edits: crate::ai::edits::EditJournal::with_db(ctx.db.clone()),
        hooks: hooks_cfg,
        turn_images: Vec::new(),
        goal_id: Some(goal.id.clone()),
        // The goal row already carries it; `current_persona` reads that.
        persona_id: None,
        read_only: false,
    };

    let messages = vec![ChatMessage::user(prompt)];
    // A persona can pin its own provider *and* its own model, so routine triage need not
    // run on the model reserved for architectural judgement — and two personas can share
    // one provider while running on different models on it.
    let choice = crate::ai::registry::ModelChoice {
        provider_id: persona.as_ref().and_then(|p| p.provider_id.clone()),
        model: persona.as_ref().and_then(|p| p.model.clone()),
    };
    let result = agent::run_turn(&tc, choice, messages, false, &tx).await;
    drop(tx);
    let _ = forward.await;

    // Re-read the goal: the agent's goal_* tool calls updated it during the turn.
    let fresh = ctx.db.get_goal(&goal.id).map_err(|e| e.to_string())?;
    let fresh = fresh.ok_or_else(|| "goal session disappeared".to_string())?;

    if let Err(e) = &result {
        crate::diag(&format!("goal {} cycle error: {e}", goal.id));
        let _ = ctx.app.emit(&event, StreamEvent::Error(e.clone()));
        // Stay active — a failed cycle is not the end of the goal.
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

    ctx.session_state.clear_cancel(&format!("goal:{goal_id}"));

    // Progress, not iterations. See `STALL_LIMIT`.
    let mut last_fingerprint: Option<u64> = None;
    let mut stalled: i64 = 0;

    loop {
        if ctx.session_state.is_cancelled(&format!("goal:{goal_id}")) {
            return;
        }
        if goal.status != "active" {
            // paused / waiting / blocked / done / stopped — stop driving.
            // The tick only resumes waiting-with-a-due-time; paused waits for Continue.
            if goal.status == "waiting" {
                if let Some(at) = goal.next_check_at.clone() {
                    let _ = ctx.app.emit(
                        &goal_event(&goal.id),
                        StreamEvent::Status(format!("waiting until {at}")),
                    );
                }
            } else if matches!(goal.status.as_str(), "done" | "blocked" | "stopped") {
                // The run is over, whichever way it ended. Anything it wrote and did not
                // commit is about to be forgotten, and "stopped" and "blocked" are the
                // cases most likely to leave a half-finished tree behind.
                secure_work(ctx, &mut goal).await;
                let _ = save_goal(ctx, &goal);
            }
            return;
        }

        match run_cycle(ctx, &goal).await {
            Ok(_) => {}
            Err(e) => {
                crate::diag(&format!("goal {goal_id} cycle failed: {e}"));
                let _ = ctx
                    .app
                    .emit(&goal_event(&goal.id), StreamEvent::Error(e.clone()));
            }
        }

        // Re-load after the cycle (the agent may have moved to waiting/done itself).
        let Some(fresh) = load_goal(&ctx.db, goal_id).ok().flatten() else {
            return;
        };
        goal = fresh;
        if goal.status != "active" {
            continue;
        }

        let cycles = goal.cycles + 1;
        goal.cycles = cycles;

        // Did that cycle do anything?
        let fingerprint = progress_fingerprint(ctx, &goal);
        if Some(fingerprint) == last_fingerprint {
            stalled += 1;
        } else {
            stalled = 0;
            last_fingerprint = Some(fingerprint);
        }
        if stalled >= STALL_LIMIT {
            goal.status = "blocked".to_string();
            goal.finished_at = Some(Utc::now().to_rfc3339());
            // Says what it did not do, because "reached max cycles" sent the user to
            // raise a number when the number was never the problem — and a higher one
            // buys nothing but more empty iterations.
            goal.outcome = Some(format!(
                "Stopped after {STALL_LIMIT} cycles in a row that changed nothing: no file \
                 edited, no board card moved, no finding recorded. It ran {cycles} cycles in \
                 total. Raising a cycle limit will not help — read what it actually did \
                 (session_read, task_audit); most often the success criteria cannot be \
                 checked with the tools it has, so it can never conclude."
            ));
            let _ = save_goal(ctx, &goal);
            notify_user(
                &ctx.app,
                "Goal stalled",
                &format!("{} stopped making progress", goal.title),
            );
            return;
        }

        // An explicit ceiling, only when the user asked for one. There is no default:
        // a task that keeps producing work is a task that should keep running.
        let spec = parse_spec(&goal);
        if let Some(max) = spec.as_ref().and_then(|s| s.max_cycles) {
            if cycles >= max {
                goal.status = "blocked".to_string();
                goal.finished_at = Some(Utc::now().to_rfc3339());
                goal.outcome = Some(format!(
                    "Stopped at the {max}-cycle limit set for this task. It was still making \
                     progress, so raise or remove the limit if the work is worth finishing."
                ));
                let _ = save_goal(ctx, &goal);
                notify_user(&ctx.app, "Goal blocked", &format!("{} reached max cycles", goal.title));
                return;
            }
        }
        let _ = save_goal(ctx, &goal);
        tokio::time::sleep(CYCLE_GAP).await;
    }
}

/// How many cycles in a row may produce nothing before the loop gives up.
///
/// Not a budget — a stall detector. A cycle that changed a file, moved a card, recorded
/// a finding or sent a message has done something, and there is no ceiling on how many
/// of those a task may take: a migration that needs two hundred is a migration that
/// needs two hundred.
///
/// What this catches is the spiral that an agent does *not* notice, because every step
/// feels new: "that did not work, let me try a slightly different approach", forever.
/// No single iteration looks like repetition, and nothing accumulates. Five in a row
/// with nothing recorded is not a busy agent, it is a stuck one.
const STALL_LIMIT: i64 = 5;

/// A fingerprint of everything a cycle could visibly accomplish.
///
/// Deliberately coarse: it is not asking whether the work was *good*, only whether
/// anything happened at all. Two identical fingerprints across a cycle mean the agent
/// moved no card, recorded no finding, and changed no file — with tools available and a
/// goal unmet, there is nothing left that would count.
fn progress_fingerprint(ctx: &GoalContext, goal: &GoalSession) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    goal.kanban_json.hash(&mut h);
    goal.memory_json.hash(&mut h);
    // Files it actually changed. The strongest signal, and the one an agent cannot
    // produce by writing a hopeful summary.
    ctx.db
        .list_file_changes(Some(&format!("goal:{}", goal.id)), None, 500)
        .map(|c| c.len())
        .unwrap_or(0)
        .hash(&mut h);
    h.finish()
}

/// Make sure a finished run did not leave work in one place only.
///
/// This is mechanical on purpose. An agent that has just declared a goal met has no
/// reason to go and look at `git status`, and the failure is invisible until the next
/// checkout throws the work away — which may be weeks later, by which time nobody knows
/// what was lost. Hoping the model remembers is not a guarantee; checking is.
///
/// Committing first and pushing second matters: when the remote or the network is the
/// broken thing, the work is still recoverable from the machine it was done on.
async fn secure_work(ctx: &GoalContext, goal: &mut GoalSession) {
    let Some(ws) = goal.workspace_id.as_deref().filter(|s| !s.is_empty()) else { return };
    let status = match crate::ai::repo::status_of(&ctx.db, &ctx.sessions, ws).await {
        Ok(s) => s,
        // Could not look. Said out loud rather than assumed fine — "we did not check"
        // and "there is nothing to save" are not the same answer.
        Err(e) => {
            crate::diag(&format!("goal {}: could not check the repository: {e}", goal.id));
            return;
        }
    };
    if !status.is_repo || !status.is_readable() || !status.work_at_risk() {
        return;
    }

    let message = format!(
        "wip({}): {}\n\nCommitted automatically when the task finished, so the work is \
         not left only on this machine.",
        goal.persona_id.as_deref().unwrap_or("agent"),
        goal.title
    );
    match crate::ai::repo::save(&ctx.db, &ctx.sessions, ws, &message).await {
        Ok(out) => {
            crate::diag(&format!("goal {}: secured work — {}", goal.id, out.trim()));
            let note = format!("Uncommitted work was committed and pushed on finishing ({}).", status.summary());
            goal.outcome = Some(match goal.outcome.take() {
                Some(o) => format!("{o}\n{note}"),
                None => note,
            });
        }
        Err(e) => {
            // The one case that must not be reported as success: the task claims to be
            // done and its changes exist nowhere but a disk nobody is watching.
            crate::diag(&format!("goal {}: COULD NOT SECURE WORK: {e}", goal.id));
            goal.status = "blocked".to_string();
            goal.outcome = Some(format!(
                "Finished, but the work could not be committed or pushed ({}): {e}. It \
                 exists only where it was done — deal with this before anything else \
                 touches that checkout.",
                status.summary()
            ));
            notify_user(
                &ctx.app,
                "Work at risk",
                &format!("{} finished with work that could not be pushed", goal.title),
            );
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
/// Start a goal's loop using whatever the app already has in managed state.
///
/// The delegation tool has an `AppHandle` and nothing else; threading `GoalRunning`,
/// `SessionManager`, `AgentHome` and the rest through `ToolContext` just to reach
/// here would put goal plumbing in front of every unrelated tool.
pub fn spawn_from_app(app: &tauri::AppHandle, goal_id: &str) {
    use tauri::Manager;
    let ctx = GoalContext {
        app: app.clone(),
        db: app.state::<Db>().inner().clone(),
        sessions: app.state::<SessionManager>().inner().clone(),
        home: app.state::<AgentHome>().inner().clone(),
        approvals: app.state::<ApprovalRegistry>().inner().clone(),
        running: app.state::<GoalRunning>().inner().clone(),
        session_state: app.state::<SessionState>().inner().clone(),
    };
    let id = goal_id.to_string();
    tauri::async_runtime::spawn(async move {
        run_loop(&ctx, &id).await;
    });
}

pub fn spawn_tick(ctx: GoalContext) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            resume_due_goals(&ctx).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stall_is_measured_in_cycles_that_changed_nothing() {
        // The rule the loop applies, stated on its own: progress resets the counter,
        // and only consecutive nothing counts. A task that produces something every
        // few cycles is working, however long it takes.
        let mut stalled = 0i64;
        let mut last: Option<u64> = None;
        // Six cycles: work, work, nothing, nothing, work, nothing.
        for fp in [1u64, 2, 2, 2, 3, 3] {
            if Some(fp) == last {
                stalled += 1;
            } else {
                stalled = 0;
                last = Some(fp);
            }
        }
        // Two runs of nothing, neither long enough, and the middle one was reset by
        // real work — so it keeps going.
        assert!(stalled < STALL_LIMIT, "stalled={stalled}");

        // Five in a row with nothing recorded is a stuck agent, not a busy one.
        let mut stalled = 0i64;
        let mut last: Option<u64> = None;
        for fp in [7u64, 7, 7, 7, 7, 7] {
            if Some(fp) == last {
                stalled += 1;
            } else {
                stalled = 0;
                last = Some(fp);
            }
        }
        assert!(stalled >= STALL_LIMIT, "stalled={stalled}");
    }
}
