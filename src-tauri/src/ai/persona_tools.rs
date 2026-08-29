//! Delegation tools: the agent hands a task to a named persona and moves on.
//!
//! Without these, background work existed but only the user could start it (`/goal`).
//! The agent could not say "this is a four-hour log audit, Ada should do it while I
//! answer the question in front of me" — it had to either do the work inline, keeping
//! the user waiting, or hand it back to them.
//!
//! A delegated task is an ordinary goal session with a persona attached, so it
//! inherits the loop that already exists: plan → act → verify, a live kanban, cycle
//! limits, pause/stop, and a notification when it finishes.

use serde_json::{json, Value};
use tauri::Emitter;

use crate::ai::provider::ToolDef;
use crate::ai::tools::ToolContext;
use crate::storage::models::{GoalSession, GoalSpec};

/// Ceiling on cycles for a delegated task when the caller names none.
///
/// Unbounded autonomy is the point, but an unbounded *loop* is how a stuck persona
/// burns tokens all night against a goal it can never satisfy. The user can raise it
/// per task, and a persona that hits the cap stops as "blocked" and says so rather
/// than failing silently.
const DEFAULT_MAX_CYCLES: i64 = 40;

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "agent_list".into(),
            description: "List the named agents (personas) available to take work, with their \
roles and default servers. Call this before agent_delegate if you are not sure who exists."
                .into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "agent_delegate".into(),
            description: "Hand a task to a named agent to work on in the background, and return \
immediately. Use this for work that is long-running, independent of what the user is asking \
right now, or better suited to another agent's remit — an audit, a migration, a watch. The \
agent runs plan/act/verify cycles on its own and notifies the user when it is done. Do NOT use \
it for something you can finish in this turn. You may name the agent, or omit it and let the \
task be routed to whoever fits."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "Persona name (e.g. \"Ada\") or id. OPTIONAL — leave it out and the task is routed to whichever agent's remit fits best."},
                    "task": {"type": "string", "description": "What to achieve, in full. The agent does not see this conversation, so include the context it needs."},
                    "success_criteria": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "How the agent will know it is finished. Without these it cannot conclude 'done'."
                    },
                    "vps_ids": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Servers to work on. Defaults to the persona's own servers."
                    },
                    "max_cycles": {"type": "integer", "description": "Cycle ceiling before it stops as blocked (default 40)."}
                },
                "required": ["task"]
            }),
        },
        ToolDef {
            name: "agent_send".into(),
            description: "Send a message to another agent — ask a question, hand over a finding, \
give an instruction. Use agent_report instead when reporting upward to your own manager. \
The recipient reads it at the start of its next cycle."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "to": {"type": "string", "description": "Recipient agent name or id."},
                    "body": {"type": "string", "description": "The message. The recipient does not see your conversation, so include the context."}
                },
                "required": ["to", "body"]
            }),
        },
        ToolDef {
            name: "agent_report".into(),
            description: "Report to your manager: progress, a result, a blocker, or a question \
only the user can answer. If you have no manager you answer to the user directly, and this \
message reaches them. THIS IS HOW YOU ESCALATE — a message to the user goes up the chain, it \
does not jump to them."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "body": {"type": "string", "description": "What you want your manager to know or decide."}
                },
                "required": ["body"]
            }),
        },
        ToolDef {
            name: "agent_inbox".into(),
            description: "Read messages other agents have sent you, and mark them read. Check \
this when you start work and after finishing a step — a colleague may have answered a question \
or changed what you should do."
                .into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "agent_thread".into(),
            description: "Read the conversation between the agents: who asked whom for what and \
what came back. Use it to catch up on a delegated task without re-running it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "Limit to one delegated task. Omit for everything recent."},
                    "limit": {"type": "integer", "description": "Max messages (default 30)."}
                }
            }),
        },
        ToolDef {
            name: "agent_hire".into(),
            description: "Create a new named agent, or change an existing one. Use this when the user describes a role they want on the team (\"add a reviewer who only reads\", \"give Ada the \
staging box\"). Naming an existing agent updates it instead of creating a second one. The user approves the change on the desktop, so say what you are about to set up before calling it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "What the agent is called, e.g. \"Ada\". Names are how everyone addresses it, so they must be unique."},
                    "role": {"type": "string", "description": "One line on what it is for. This is what task routing matches against, so make it describe the work, not the personality."},
                    "instructions": {"type": "string", "description": "Standing instructions for every run of this agent."},
                    "reports_to": {"type": "string", "description": "The agent this one escalates to, by name. Omit for one that answers to the user directly."},
                    "vps_ids": {"type": "array", "items": {"type": "string"}, "description": "Servers it works on by default."},
                    "safety_mode": {"type": "string", "enum": ["approve", "allowlist", "full"], "description": "How much it may do unattended. Omit to use the global setting."},
                    "enabled": {"type": "boolean", "description": "Whether it may be given work. Default true."}
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "agent_dismiss".into(),
            description: "Delete a named agent. Its finished work and the conversation it took part in are kept — only the agent itself goes. Ask the user first; this is not something to do on your own initiative."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "Name or id of the agent to remove."}
                },
                "required": ["agent"]
            }),
        },
        ToolDef {
            name: "agent_org".into(),
            description: "Show the reporting structure: who answers to whom, and who answers to the user. Call it before changing a reporting line, so you can see what the change would do."
                .into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "project_history".into(),
            description: "Everything this project has to show for itself: the tasks that were delegated, what the agents said to each other, the files that changed, and the commits that came out of it. Use it to catch up on a project rather than asking the user what happened. It covers the project currently open — other projects' work is deliberately not included."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "description": "Max messages and file changes to include (default 40)."}
                }
            }),
        },
        ToolDef {
            name: "agent_check".into(),
            description: "Check on delegated tasks: which agent is on what, the state of their \
board, and results from finished ones. With no id, lists every delegated task."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The id returned by agent_delegate. Omit to list all."}
                }
            }),
        },
    ]
}

pub fn is_persona_tool(name: &str) -> bool {
    matches!(
        name,
        "agent_list"
            | "agent_delegate"
            | "agent_check"
            | "agent_send"
            | "agent_report"
            | "agent_inbox"
            | "agent_thread"
            | "agent_hire"
            | "agent_dismiss"
            | "agent_org"
            | "project_history"
    )
}

/// Reading who exists, how a task is going, or what the agents have said changes
/// nothing. Delegating starts real work on real servers, and sending a message can
/// cause another agent to act — so plan mode, where the user has said "not yet", must
/// withhold both.
pub fn tool_is_mutating(name: &str) -> bool {
    matches!(
        name,
        "agent_delegate" | "agent_send" | "agent_report" | "agent_hire" | "agent_dismiss"
    )
}

pub async fn dispatch(ctx: &ToolContext, name: &str, args: &Value) -> String {
    match name {
        "agent_list" => agent_list(ctx),
        "agent_delegate" => agent_delegate(ctx, args),
        "agent_check" => agent_check(ctx, args),
        "agent_send" => agent_send(ctx, args),
        "agent_report" => agent_report(ctx, args),
        "agent_inbox" => agent_inbox(ctx),
        "agent_thread" => agent_thread(ctx, args),
        "agent_hire" => agent_hire(ctx, args).await,
        "agent_dismiss" => agent_dismiss(ctx, args).await,
        "agent_org" => agent_org(ctx),
        "project_history" => project_history(ctx, args).await,
        _ => format!("error: unknown persona tool {name}"),
    }
}

fn agent_list(ctx: &ToolContext) -> String {
    match ctx.db.list_personas() {
        Ok(list) => format!(
            "Named agents you can delegate to:\n{}",
            crate::ai::persona::format_catalog(&list)
        ),
        Err(e) => format!("error listing agents: {e}"),
    }
}

fn agent_delegate(ctx: &ToolContext, args: &Value) -> String {
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("").trim();
    if task.is_empty() {
        return "error: missing 'task'".into();
    }
    let requested = args.get("agent").and_then(|v| v.as_str()).unwrap_or("").trim();
    let known = ctx.db.list_personas().unwrap_or_default();

    // Named agent wins; otherwise route on remit, the way an agent that hits something
    // outside its scope forwards to whoever's description fits. Declining beats
    // guessing — a task sent to the wrong agent is worse than one that asks who.
    let (persona, routed) = if requested.is_empty() {
        match crate::ai::persona::best_match(&known, task) {
            Some(p) => (p.clone(), true),
            None => {
                return format!(
                    "error: no agent's remit matches that task, so name one explicitly.\n{}",
                    crate::ai::persona::format_catalog(&known)
                )
            }
        }
    } else {
        match crate::ai::persona::resolve(&ctx.db, requested) {
            Some(p) => (p, false),
            None => {
                return format!(
                    "error: no agent named {requested:?}.\n{}",
                    crate::ai::persona::format_catalog(&known)
                )
            }
        }
    };
    if !persona.enabled {
        return format!("error: agent {} is disabled; enable it in Settings → Agents first", persona.name);
    }

    let requested_targets: Vec<String> = args
        .get("vps_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    // A delegated task must not reach servers this session was not given. The persona's
    // own defaults are user-configured and so are trusted; ids the model supplies are
    // only honoured if this session already holds them.
    let requested_targets: Vec<String> = requested_targets
        .into_iter()
        .filter(|id| ctx.targets.iter().any(|t| t == id))
        .collect();
    let targets = crate::ai::persona::effective_targets(Some(&persona), &requested_targets);

    let success_criteria: Vec<String> = args
        .get("success_criteria")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let max_cycles = args
        .get("max_cycles")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_CYCLES);

    let spec = GoalSpec {
        objective: task.to_string(),
        success_criteria,
        check_method: "Verify with tools against the servers before claiming done.".into(),
        check_tooling: vec![],
        hard_constraints: vec![],
        max_cycles: Some(max_cycles),
        vps_targets: targets.clone(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let goal = GoalSession {
        id: id.clone(),
        title: title_for(&persona.name, task),
        raw_request: task.to_string(),
        spec_json: serde_json::to_string(&spec).unwrap_or_else(|_| "{}".into()),
        // Straight to active: the user already approved by asking for the work, and a
        // delegated task that sat in "intake" waiting for a second confirmation would
        // reintroduce exactly the interruption delegation exists to remove.
        status: "active".into(),
        kanban_json: "[]".into(),
        memory_json: "{}".into(),
        next_check_at: None,
        cycles: 0,
        created_at: None,
        updated_at: None,
        finished_at: None,
        persona_id: Some(persona.id.clone()),
        // The delegated agent inherits the project it was delegated from. It is the
        // only way it can know which codebase the objective is about — it does not see
        // this conversation, and an objective without a project is a guess.
        workspace_id: ctx.workspace_id.clone(),
    };
    if let Err(e) = ctx.db.insert_goal(&goal) {
        return format!("error creating delegated task: {e}");
    }
    crate::ai::goal::spawn_from_app(&ctx.app, &id);

    let where_ = match targets.len() {
        0 => "no servers selected — it will ask if it needs one".to_string(),
        n => format!("{n} server(s)"),
    };
    let how = if routed {
        format!(" (routed to {} — their remit fit the task)", persona.name)
    } else {
        String::new()
    };
    format!(
        "Delegated to {name}{how} (task_id {id}, {where_}, up to {max_cycles} cycles).\n\
         {name} is working on it in the background now and the user is notified when it \
         finishes. Do not wait for it — carry on with what the user asked. Use \
         agent_check(task_id: \"{id}\") to see progress.",
        name = persona.name,
        how = how,
    )
}

fn agent_check(ctx: &ToolContext, args: &Value) -> String {
    let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("").trim();
    if id.is_empty() {
        let goals = match ctx.db.list_goals() {
            Ok(g) => g,
            Err(e) => return format!("error listing delegated tasks: {e}"),
        };
        let delegated: Vec<&GoalSession> =
            goals.iter().filter(|g| g.persona_id.is_some()).collect();
        if delegated.is_empty() {
            return "No delegated tasks.".into();
        }
        let lines: Vec<String> = delegated
            .iter()
            .map(|g| {
                let who = g
                    .persona_id
                    .as_deref()
                    .and_then(|p| crate::ai::persona::resolve(&ctx.db, p))
                    .map(|p| p.name)
                    .unwrap_or_else(|| "(deleted agent)".into());
                format!("- {} [{}] {} — {}", g.id, g.status, who, g.title)
            })
            .collect();
        return format!("Delegated tasks:\n{}", lines.join("\n"));
    }

    let Ok(Some(goal)) = ctx.db.get_goal(id) else {
        return format!("No delegated task {id}.");
    };
    let who = goal
        .persona_id
        .as_deref()
        .and_then(|p| crate::ai::persona::resolve(&ctx.db, p))
        .map(|p| p.name)
        .unwrap_or_else(|| "the default agent".into());
    let board = crate::ai::goal::parse_kanban(&goal);
    let cards = if board.is_empty() {
        "(no cards yet)".to_string()
    } else {
        board
            .iter()
            .map(|t| {
                let result = t.result.clone().unwrap_or_default();
                format!("  [{}] {} {}", t.column, t.title, result)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{who} — {} ({}, {} cycle(s))\nObjective: {}\n{cards}",
        goal.title, goal.status, goal.cycles, goal.raw_request
    )
}

// ---------------------------------------------------------------------------
// Messaging between agents
// ---------------------------------------------------------------------------

/// The persona whose turn this is, derived from the goal the cycle belongs to.
///
/// A tool call has no idea who is running it; the goal session does. Without this,
/// `agent_report` could not tell who is reporting to whom, and the main chat agent
/// (which has no persona) would appear to be a nameless employee of someone.
fn current_persona(ctx: &ToolContext) -> Option<crate::storage::models::Persona> {
    let goal_id = ctx.goal_id.clone().or_else(|| {
        ctx.session_id
            .strip_prefix("goal:")
            .map(|s| s.to_string())
    })?;
    let goal = ctx.db.get_goal(&goal_id).ok().flatten()?;
    let persona_id = goal.persona_id?;
    ctx.db.get_persona(&persona_id).ok().flatten()
}

fn display_name(ctx: &ToolContext, id: Option<&str>) -> String {
    match id {
        None => "the user".into(),
        Some(id) => ctx
            .db
            .get_persona(id)
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| "(deleted agent)".into()),
    }
}

fn record_message(
    ctx: &ToolContext,
    from_id: Option<String>,
    to_id: Option<String>,
    kind: &str,
    body: &str,
) -> Result<(), String> {
    let msg = crate::storage::models::AgentMessage {
        id: uuid::Uuid::new_v4().to_string(),
        from_id,
        to_id: to_id.clone(),
        kind: kind.to_string(),
        body: body.to_string(),
        goal_id: ctx.goal_id.clone().or_else(|| {
            ctx.session_id
                .strip_prefix("goal:")
                .map(|s| s.to_string())
        }),
        // Stamped with the project it was said about, so one agent's thread on one
        // codebase never arrives in the middle of another's.
        workspace_id: ctx.workspace_id.clone(),
        read_at: None,
        created_at: None,
    };
    ctx.db
        .insert_agent_message(&msg)
        .map_err(|e| e.to_string())?;
    // The UI mirrors the exchange live, so the user can watch the agents talk rather
    // than discovering afterwards that they did.
    let _ = ctx.app.emit("agent://message", &msg);
    Ok(())
}

fn agent_send(ctx: &ToolContext, args: &Value) -> String {
    let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("").trim();
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
    if to.is_empty() || body.is_empty() {
        return "error: agent_send needs both 'to' and 'body'".into();
    }
    let Some(recipient) = crate::ai::persona::resolve(&ctx.db, to) else {
        let known = ctx.db.list_personas().unwrap_or_default();
        return format!(
            "error: no agent named {to:?}.\n{}",
            crate::ai::persona::format_catalog(&known)
        );
    };
    let me = current_persona(ctx);
    let from_name = me.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "the main agent".into());
    if let Err(e) = record_message(
        ctx,
        me.as_ref().map(|p| p.id.clone()),
        Some(recipient.id.clone()),
        "request",
        body,
    ) {
        return format!("error sending message: {e}");
    }
    format!(
        "Message sent from {from_name} to {}. They read it at the start of their next cycle — \
         do not wait for a reply, carry on and check agent_inbox later.",
        recipient.name
    )
}

fn agent_report(ctx: &ToolContext, args: &Value) -> String {
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
    if body.is_empty() {
        return "error: agent_report needs a 'body'".into();
    }
    let Some(me) = current_persona(ctx) else {
        // The main chat agent is already talking to the user; it has no manager and
        // nothing to escalate to.
        return "You are talking to the user directly — say it in your reply rather than \
                reporting it."
            .into();
    };
    let all = ctx.db.list_personas().unwrap_or_default();
    let manager = crate::ai::persona::manager_of(&all, &me).map(|m| (m.id.clone(), m.name.clone()));
    let (to_id, to_name) = match manager {
        Some((id, name)) => (Some(id), name),
        // No manager: this persona answers to the user, so the report reaches them.
        None => (None, "the user".to_string()),
    };
    if let Err(e) = record_message(ctx, Some(me.id.clone()), to_id.clone(), "report", body) {
        return format!("error reporting: {e}");
    }
    if to_id.is_none() {
        let _ = ctx.app.emit(
            "goal://notify",
            json!({ "title": format!("{} reported", me.name), "body": body }),
        );
    }
    format!("Reported to {to_name}.")
}

fn agent_inbox(ctx: &ToolContext) -> String {
    let me = current_persona(ctx);
    let my_id = me.as_ref().map(|p| p.id.clone());
    let ws = ctx.workspace_id.as_deref().filter(|s| !s.is_empty());
    let msgs = match ctx.db.unread_agent_messages(my_id.as_deref(), ws) {
        Ok(m) => m,
        Err(e) => return format!("error reading inbox: {e}"),
    };
    // Scoping the inbox to one project means the others stop appearing, which is the
    // point — but silently. Saying how many are waiting elsewhere is what keeps that
    // from reading as "my message vanished".
    let elsewhere = ctx
        .db
        .unread_agent_messages_elsewhere(my_id.as_deref(), ws)
        .unwrap_or(0);
    let footer = match elsewhere {
        0 => String::new(),
        n => format!(
            "\n\n({n} more unread in other projects — switch project to read them.)"
        ),
    };
    if msgs.is_empty() {
        return format!("No new messages in this project.{footer}");
    }
    let lines: Vec<String> = msgs
        .iter()
        .map(|m| {
            format!(
                "[{}] from {}: {}",
                m.kind,
                display_name(ctx, m.from_id.as_deref()),
                m.body
            )
        })
        .collect();
    let ids: Vec<String> = msgs.iter().map(|m| m.id.clone()).collect();
    // Marked read on delivery: an unread message the agent has already acted on would
    // otherwise be re-delivered every cycle and re-actioned every time.
    if let Err(e) = ctx.db.mark_agent_messages_read(&ids) {
        crate::diag(&format!("agent inbox: could not mark read: {e}"));
    }
    format!("{} new message(s):\n{}{footer}", msgs.len(), lines.join("\n"))
}

fn agent_thread(ctx: &ToolContext, args: &Value) -> String {
    let goal_id = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(30);
    let ws = ctx.workspace_id.as_deref().filter(|s| !s.is_empty());
    let msgs = match ctx.db.list_agent_messages(goal_id, ws, limit) {
        Ok(m) => m,
        Err(e) => return format!("error reading the thread: {e}"),
    };
    if msgs.is_empty() {
        return "The agents have not said anything about this project yet.".into();
    }
    let lines: Vec<String> = msgs
        .iter()
        .map(|m| {
            format!(
                "{} -> {} [{}]: {}",
                display_name(ctx, m.from_id.as_deref()),
                display_name(ctx, m.to_id.as_deref()),
                m.kind,
                m.body
            )
        })
        .collect();
    format!("Agent conversation (oldest first):\n{}", lines.join("\n"))
}

/// Put a change to the team in front of the user, then block until they approve it.
///
/// Hiring an agent is not a settings tweak: a persona carries its own safety mode and
/// its own servers, so a created agent is a standing grant of access. The agent reads
/// files and logs it did not write, any of which can ask it to create one — so this
/// ignores the session's safety mode and always prompts.
async fn confirm(ctx: &ToolContext, summary: &str) -> Result<(), String> {
    crate::ai::safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        "approve",
        &ctx.session_id,
        None,
        summary,
    )
    .await
}

async fn agent_hire(ctx: &ToolContext, args: &Value) -> String {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return "error: agent_hire needs a 'name'".into();
    }
    // Naming an existing agent updates it. Creating a second one with the same name
    // would make `ask Ada to…` ambiguous, and `resolve` would pick whichever the
    // database returned first.
    let existing = crate::ai::persona::resolve(&ctx.db, name);
    let str_arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::trim);

    let reports_to = match str_arg("reports_to").filter(|s| !s.is_empty()) {
        Some(mgr) => match crate::ai::persona::resolve(&ctx.db, mgr) {
            Some(p) => Some(p.id),
            None => {
                let known = ctx.db.list_personas().unwrap_or_default();
                return format!(
                    "error: no agent named {mgr:?} to report to.\n{}",
                    crate::ai::persona::format_catalog(&known)
                );
            }
        },
        // Absent means "unchanged" on an update, and "answers to the user" on a
        // creation — clearing a reporting line has to be asked for, not inferred from
        // a field the caller simply did not mention.
        None => existing.as_ref().and_then(|p| p.reports_to.clone()),
    };

    let input = crate::storage::models::PersonaInput {
        id: existing.as_ref().map(|p| p.id.clone()),
        name: name.to_string(),
        role: str_arg("role")
            .map(str::to_string)
            .or_else(|| existing.as_ref().map(|p| p.role.clone()))
            .unwrap_or_default(),
        instructions: str_arg("instructions")
            .map(str::to_string)
            .or_else(|| existing.as_ref().map(|p| p.instructions.clone()))
            .unwrap_or_default(),
        targets: args
            .get("vps_ids")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .or_else(|| existing.as_ref().map(|p| p.targets.clone()))
            .unwrap_or_default(),
        safety_mode: str_arg("safety_mode")
            .map(str::to_string)
            .or_else(|| existing.as_ref().and_then(|p| p.safety_mode.clone())),
        provider_id: existing.as_ref().and_then(|p| p.provider_id.clone()),
        model: existing.as_ref().and_then(|p| p.model.clone()),
        enabled: args
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| existing.as_ref().map(|p| p.enabled).unwrap_or(true)),
        reports_to,
    };

    let manager_name = input
        .reports_to
        .as_deref()
        .map(|id| display_name(ctx, Some(id)))
        .unwrap_or_else(|| "the user".into());
    let summary = format!(
        "{} the agent \"{}\".\n\
         Role: {}\n\
         Reports to: {}\n\
         Servers: [{}]   Trust: {}   {}",
        if existing.is_some() { "Update" } else { "Create" },
        input.name,
        if input.role.is_empty() { "(none given)" } else { &input.role },
        manager_name,
        input.targets.join(", "),
        input.safety_mode.clone().unwrap_or_else(|| "global default".into()),
        if input.enabled { "May be given work." } else { "Disabled." },
    );
    if let Err(e) = confirm(ctx, &summary).await {
        return format!("not changed: {e}");
    }

    // Through the command layer's own validation, so a tool call cannot create a
    // duplicate name or a reporting loop the settings screen would have refused.
    match crate::commands::persona::save_persona_checked(&ctx.db, input) {
        Ok(p) => format!(
            "{} {}.\n\n{}",
            if existing.is_some() { "Updated" } else { "Created" },
            p.name,
            crate::ai::persona::format_org_chart(&ctx.db.list_personas().unwrap_or_default())
        ),
        Err(e) => format!("error: {e}"),
    }
}

async fn agent_dismiss(ctx: &ToolContext, args: &Value) -> String {
    let who = args.get("agent").and_then(|v| v.as_str()).unwrap_or("").trim();
    let Some(p) = crate::ai::persona::resolve(&ctx.db, who) else {
        let known = ctx.db.list_personas().unwrap_or_default();
        return format!(
            "error: no agent named {who:?}.\n{}",
            crate::ai::persona::format_catalog(&known)
        );
    };
    let all = ctx.db.list_personas().unwrap_or_default();
    let orphans = crate::ai::persona::reports_of(&all, &p);
    let summary = format!(
        "Delete the agent \"{}\".{}\n\
         Their finished work and the conversation they took part in are kept.",
        p.name,
        if orphans.is_empty() {
            String::new()
        } else {
            // Said before the deletion, not discovered after it: whoever reported to
            // this agent is about to start escalating somewhere else.
            format!(
                "\n{} would then report to the user directly: {}",
                orphans.len(),
                orphans.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(", ")
            )
        }
    );
    if let Err(e) = confirm(ctx, &summary).await {
        return format!("not deleted: {e}");
    }
    match ctx.db.delete_persona(&p.id) {
        Ok(()) => format!("Deleted {}.", p.name),
        Err(e) => format!("error deleting {}: {e}", p.name),
    }
}

fn agent_org(ctx: &ToolContext) -> String {
    let all = ctx.db.list_personas().unwrap_or_default();
    if all.is_empty() {
        return "There are no named agents yet. Use agent_hire to create one.".into();
    }
    crate::ai::persona::format_org_chart(&all)
}

/// One project's record, for an agent catching up rather than asking the user.
async fn project_history(ctx: &ToolContext, args: &Value) -> String {
    let Some(ws) = ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) else {
        return "No project is open, so there is no project history to read. \
                agent_thread shows the conversation across everything."
            .into();
    };
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(40);
    let h = match crate::commands::project::history(&ctx.db, &ctx.sessions, ws, limit).await {
        Ok(h) => h,
        Err(e) => return format!("error reading the project history: {e}"),
    };

    let mut out = format!("Project: {}", h.name);
    if let Some(loc) = &h.location {
        out.push_str(&format!(" ({loc})"));
    }
    if let Some(branch) = &h.branch {
        out.push_str(&format!(" on {branch}"));
    }
    out.push_str("\n");

    out.push_str(&format!("\nDelegated tasks ({}):\n", h.tasks.len()));
    for t in h.tasks.iter().take(20) {
        out.push_str(&format!(
            "- [{}] {} ({} cycles)\n",
            t.status,
            t.title,
            t.cycles
        ));
    }
    if h.tasks.is_empty() {
        out.push_str("- (none)\n");
    }

    out.push_str(&format!("\nWhat the agents said ({}):\n", h.messages.len()));
    for m in h.messages.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
        out.push_str(&format!(
            "- {} -> {} [{}]: {}\n",
            display_name(ctx, m.from_id.as_deref()),
            display_name(ctx, m.to_id.as_deref()),
            m.kind,
            m.body.lines().next().unwrap_or("").chars().take(160).collect::<String>()
        ));
    }
    if h.messages.is_empty() {
        out.push_str("- (nothing yet)\n");
    }

    out.push_str(&format!("\nFiles changed ({}):\n", h.changes.len()));
    for c in h.changes.iter().take(20) {
        out.push_str(&format!(
            "- {} {} ({})\n",
            if c.is_new { "created" } else { "edited" },
            c.path,
            c.label
        ));
    }
    if h.changes.is_empty() {
        out.push_str("- (none)\n");
    }

    match (&h.git_note, h.commits.is_empty()) {
        (Some(note), _) => out.push_str(&format!("\nCommits: {note}\n")),
        (None, true) => out.push_str("\nCommits: none yet\n"),
        (None, false) => {
            out.push_str(&format!("\nRecent commits ({}):\n", h.commits.len()));
            for c in h.commits.iter().take(15) {
                out.push_str(&format!("- {} {} — {}\n", c.sha, c.subject, c.author));
            }
        }
    }
    out
}

/// A short board title: who is doing it and the first clause of what.
fn title_for(persona: &str, task: &str) -> String {
    let first = task.lines().next().unwrap_or(task).trim();
    let short: String = first.chars().take(40).collect();
    let ellipsis = if first.chars().count() > 40 { "…" } else { "" };
    format!("{persona}: {short}{ellipsis}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_delegation_counts_as_mutating() {
        // Listing agents and reading a board change nothing, so plan mode can allow
        // them; starting real work on real servers it must not.
        assert!(tool_is_mutating("agent_delegate"));
        assert!(!tool_is_mutating("agent_list"));
        assert!(!tool_is_mutating("agent_check"));
    }

    #[test]
    fn the_three_tools_are_recognised() {
        for n in ["agent_list", "agent_delegate", "agent_check"] {
            assert!(is_persona_tool(n), "{n}");
        }
        assert!(!is_persona_tool("run_command"));
    }

    #[test]
    fn every_definition_is_dispatchable() {
        // A tool the model can see but not call is worse than no tool at all.
        for d in definitions() {
            assert!(is_persona_tool(&d.name), "undispatchable: {}", d.name);
        }
    }

    #[test]
    fn titles_name_the_agent_and_stay_short() {
        assert_eq!(title_for("Ada", "check the logs"), "Ada: check the logs");
        let long = title_for("Ada", &"x".repeat(100));
        assert!(long.starts_with("Ada: "), "{long}");
        assert!(long.ends_with('…'), "{long}");
        assert!(long.chars().count() <= 46, "{long}");
        // A multi-line task uses only its first line.
        assert_eq!(title_for("CEO", "audit\nthen report"), "CEO: audit");
    }
}
