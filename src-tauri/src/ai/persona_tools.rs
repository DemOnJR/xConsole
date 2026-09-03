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

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::ai::provider::ToolDef;
use crate::ai::tools::ToolContext;
use crate::storage::models::{GoalSession, GoalSpec, Persona};
use crate::storage::Db;

/// Marker in a standing-duty objective so the ticker can tell assigned work from
/// "you were idle, go do your job".
pub const DUTY_MARK: &str = "[standing-duty]";

/// How long after a run we leave someone alone before handing them the next
/// unsolicited piece of their own remit. Unread mail bypasses this — a message
/// is work, not a suggestion.
pub const DUTY_COOLDOWN: Duration = Duration::minutes(20);

/// Cap on new standing-duty loops started in one tick. Assigned tasks still start
/// immediately; this only stops an idle office of fifteen from all waking at once
/// and burning the provider on overlapping "I should do something" cycles.
pub const MAX_DUTY_SPAWNS: usize = 3;

/// Cap on persona loops in flight (assigned + duty). New standing work waits;
/// a task the user or a lead actually handed over does not.
pub const MAX_PERSONA_LOOPS: usize = 6;

// A delegated task has no cycle ceiling by default.
//
// It used to be 40, and a count turned out to be the wrong measure entirely: forty
// cycles that each changed something is a long piece of work finishing, and four that
// changed nothing is an agent stuck. Stopping the first while letting the second run on
// is exactly backwards, and the "reached max cycles" message sent people to raise a
// number that was never the problem.
//
// The loop stops on lack of progress instead (`goal::STALL_LIMIT`). A ceiling is still
// available per task for anyone who wants a hard budget.

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
                    "max_cycles": {"type": "integer", "description": "Optional hard budget of plan/act/verify cycles. Leave it out: there is no default ceiling, and the loop already stops on its own when several cycles in a row change nothing. Only set it when you specifically want to cap what a task may spend."},
                    "project": {
                        "type": "string",
                        "description": "Which project this is about, by name. Defaults to the one currently open. Give it when the user asks about a project that is not open — that is how one conversation reaches every team. Use teams_overview to see the names."
                    }
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
                    "enabled": {"type": "boolean", "description": "Whether it may be given work. Default true."},
                    "project": {"type": "string", "description": "The project it works on, by name. An agent on a project is only reachable while that project is open, and its files and tasks are filed there. Pass \"company-wide\" for the few that answer about everything. Say this when you are not working inside a project, or the agent ends up company-wide by accident."},
                    "rename_to": {"type": "string", "description": "New name for the agent named in 'name'. Names are how everyone addresses each other, so renaming changes who answers to what."},
                    "provider_id": {"type": "string", "description": "Which provider account it runs through, by name or id. Omit to use the active one."},
                    "model": {"type": "string", "description": "Model for this agent, e.g. a cheaper one for routine work. Omit to use the provider's."},
                    "allowed_paths": {"type": "array", "items": {"type": "string"}, "description": "Globs inside the project it may read and write: [\"src-tauri/**\"] for a backend engineer, [\"docs/**\"] for a writer. Empty means the whole project. This is the difference between a reviewer and an engineer, so set it."},
                    "allowed_tools": {"type": "array", "items": {"type": "string"}, "description": "Tool names it may call, e.g. [\"local_*\", \"repo_*\"]. Empty means every tool. Reporting and asking are never taken away."}
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "agent_inspect".into(),
            description: "Show one agent's whole configuration: role, project, who it reports to, \
its servers, trust, provider and model, which paths and tools it is limited to, and what it is \
working on now. Read this before changing an agent — agent_hire overwrites what you pass and \
leaves the rest, so knowing what is there is how you avoid widening something by accident."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "Name or id of the agent."}
                },
                "required": ["agent"]
            }),
        },
        ToolDef {
            name: "team_create".into(),
            description: "Set up a whole team for a project in one go, with the reporting line \
already right: a lead that answers to the user, and the rest answering to the lead. Use it when \
a project has nobody on it — building a team one agent at a time is where people give up. \
Names are prefixed with the project, so several projects can each have a lead. Agents that \
already exist are left alone."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."},
                    "roles": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Roles to create, e.g. [\"lead\", \"engineer\", \"reviewer\", \"ops\"]. Defaults to lead, engineer and reviewer. The first one is the lead and answers to the user."
                    },
                    "about": {"type": "string", "description": "One line on what the project is, put into every member's instructions so they know what they are working on."}
                }
            }),
        },
        ToolDef {
            name: "task_stop".into(),
            description: "Stop a running task for good. This is how you intervene when one of \
your agents has gone off the rails — the wrong file, the wrong server, the same failing step \
five cycles running. Stopping is not a punishment: the work done so far is kept, and you can \
delegate a corrected version afterwards. Use agent_check or teams_overview to get the task_id."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The task to stop."},
                    "reason": {"type": "string", "description": "Why, in one line. It goes into your report, so the user can see what you stopped and why."}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "task_pause".into(),
            description: "Halt a running task where it stands, keeping everything it has done. \
Use it when you need to look before deciding — a task you might resume. task_resume restarts \
it from where it stopped; task_stop ends it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The task to pause."},
                    "reason": {"type": "string", "description": "Why you are pausing it."}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "task_resume".into(),
            description: "Start a paused, waiting or blocked task running again, from where it \
left off. Say what changed first — the agent picks up its own board, not your reasoning."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The task to continue."},
                    "note": {"type": "string", "description": "What changed, sent to the agent so it knows why it is running again."}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "task_reassign".into(),
            description: "Hand a task to a different agent, keeping its board and everything \
already done. Use it when the work turned out to be somebody else's — a deployment problem \
that is really a database problem — instead of stopping it and starting again from nothing. \
Only agents on the same project can take it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The task to hand over."},
                    "to": {"type": "string", "description": "The agent taking it, by name."},
                    "note": {"type": "string", "description": "What the new owner needs to know: what has been tried, and why it is theirs now."}
                },
                "required": ["task_id", "to"]
            }),
        },
        ToolDef {
            name: "feature_propose".into(),
            description: "Ask permission to build something that does not exist yet. Fixing, \
improving and finishing what is already there is your job and needs nobody's agreement — this \
is only for new surface area: a new page, a new command, a new integration, a new table. Your \
manager reads it, then theirs, up to whoever answers to the user. While it is undecided you may \
write documentation and nothing else, so say what you intend rather than building it first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "The feature in one line."},
                    "body": {"type": "string", "description": "What it is, why now, what it would cost to keep, and what you would NOT do. Write it for somebody deciding on a phone."}
                },
                "required": ["title", "body"]
            }),
        },
        ToolDef {
            name: "feature_decide".into(),
            description: "Approve or refuse a proposed feature. Use it when the user has told \
you what they want to happen, or when the decision is genuinely yours. Approving unblocks the \
agent that asked; refusing closes it with your reason, which they read."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "proposal_id": {"type": "string", "description": "From feature_list."},
                    "decision": {"type": "string", "enum": ["approve", "reject"], "description": "What happens to it."},
                    "note": {"type": "string", "description": "Why. The proposer reads this, so make it usable."}
                },
                "required": ["proposal_id", "decision"]
            }),
        },
        ToolDef {
            name: "feature_list".into(),
            description: "Proposed features and what became of them. Call it when asked what \
the team wants to build, or before deciding, so you are not answering the same proposal twice."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "state": {"type": "string", "enum": ["proposed", "approved", "rejected"], "description": "Only these. Default: everything still open."},
                    "project": {"type": "string", "description": "Project name. Defaults to the one you are working on."},
                    "limit": {"type": "integer", "description": "Max rows (default 20)."}
                }
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
            name: "review_schedule".into(),
            description: "Set up (or change, or stop) a recurring review of a project, run by \
one of its agents. This is what makes a team look after a project while nobody is asking: on \
the schedule, that agent reads the project's numbers and its own team's work, decides what to \
change, and reports up. Use it when the user says a project should be kept an eye on."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."},
                    "agent": {"type": "string", "description": "Which agent runs it — normally the lead of that project. Defaults to the project's agent that reports to the user."},
                    "schedule": {"type": "string", "description": "\"@daily HH:MM\", \"@weekly mon HH:MM\", \"@hourly\", or \"@every 6h\". Default \"@weekly mon 09:00\"."},
                    "enabled": {"type": "boolean", "description": "False stops an existing review without deleting it."},
                    "focus": {"type": "string", "description": "Anything specific this review should always check, on top of the standard briefing."}
                }
            }),
        },
        ToolDef {
            name: "project_review".into(),
            description: "The whole picture for one project in one call: how its numbers moved, \
what each agent on it did and what came of that, what changed on the servers, and what is still \
open. This is the briefing for deciding what the team should do next — run it before changing \
anyone's remit, and on a schedule so a project is reviewed even when nobody asks."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."},
                    "days": {"type": "integer", "description": "Period to review, in days (default 7)."}
                }
            }),
        },
        ToolDef {
            name: "task_audit".into(),
            description: "Check a finished task's report against what actually happened: the \
commands its session ran, the files it changed, and whether the work was committed. Use it \
before believing a result you are going to build on, and on anything a team member reports as \
done. It flags the combinations that do not add up — 'done' with nothing changed, or a claim of \
testing with no test having run."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The delegated task to audit."}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "agent_activity".into(),
            description: "What one agent has actually done over the last N days: the tasks it \
was given and how each ended, the files it changed, and what it said to the rest of the team. \
Use it to answer \"what has X been doing?\", to review a team member before changing their \
remit, and when deciding whether the work is producing results."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "Agent name or id."},
                    "days": {"type": "integer", "description": "How far back to look (default 7)."}
                },
                "required": ["agent"]
            }),
        },
        ToolDef {
            name: "teams_overview".into(),
            description: "Every project, the team on it, and what each team is working on \
right now. This is how you answer \"what is happening?\" across everything without opening \
each project in turn, and how you decide which team a request belongs to. Call it before \
delegating something that is not about the project currently open."
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
            | "agent_inspect"
            | "team_create"
            | "agent_dismiss"
            | "agent_org"
            | "task_stop"
            | "task_pause"
            | "task_resume"
            | "task_reassign"
            | "feature_propose"
            | "feature_decide"
            | "feature_list"
            | "teams_overview"
            | "agent_activity"
            | "task_audit"
            | "project_review"
            | "review_schedule"
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
        "agent_delegate"
            | "agent_send"
            | "agent_report"
            | "agent_hire"
            | "team_create"
            | "agent_dismiss"
            | "review_schedule"
            | "task_stop"
            | "task_pause"
            | "task_resume"
            | "task_reassign"
            | "feature_propose"
            | "feature_decide"
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
        "agent_inspect" => agent_inspect(ctx, args),
        "team_create" => team_create(ctx, args).await,
        "agent_dismiss" => agent_dismiss(ctx, args).await,
        "agent_org" => agent_org(ctx),
        "task_stop" => task_stop(ctx, args),
        "task_pause" => task_pause(ctx, args),
        "task_resume" => task_resume(ctx, args),
        "task_reassign" => task_reassign(ctx, args),
        "feature_propose" => feature_propose(ctx, args).await,
        "feature_decide" => feature_decide(ctx, args),
        "feature_list" => feature_list(ctx, args),
        "teams_overview" => teams_overview(ctx),
        "agent_activity" => agent_activity(ctx, args),
        "task_audit" => task_audit(ctx, args).await,
        "project_review" => project_review(ctx, args).await,
        "review_schedule" => review_schedule(ctx, args).await,
        "project_history" => project_history(ctx, args).await,
        _ => format!("error: unknown persona tool {name}"),
    }
}

/// Look one named agent up among the ones this call may address.
///
/// `crate::ai::persona::resolve` searches the whole database, which is right for the
/// settings screen and wrong here: it let an agent on one project hand work to another
/// project's engineer by typing their name, straight past the team list built one line
/// above the call. `known` is that list, and this is what makes it mean something.
///
/// `scoped` is false only for a turn with no project at all — the agent the user talks
/// to over chat, which is the one that has to be able to reach anybody. When it does,
/// the answer carries that agent's own project so the work is still filed correctly.
pub(crate) fn find_addressable(
    known: &[Persona],
    all: &[Persona],
    requested: &str,
    scoped: bool,
) -> Result<Persona, String> {
    let needle = requested.trim();
    let matches = |p: &Persona| p.id == needle || p.name.eq_ignore_ascii_case(needle);
    if let Some(p) = known.iter().find(|p| matches(p)) {
        return Ok(p.clone());
    }
    if !scoped {
        if let Some(p) = all.iter().find(|p| matches(p)) {
            return Ok(p.clone());
        }
        return Err(format!(
            "error: no agent named {requested:?}.\n{}",
            crate::ai::persona::format_catalog(known)
        ));
    }
    // They exist, but not here. Saying so is the whole value of the refusal: the caller
    // learns to ask their manager rather than retrying the name.
    if let Some(elsewhere) = all.iter().find(|p| matches(p)) {
        return Err(format!(
            "error: {} works on another project, not this one. Work does not cross \
             projects by name — ask their manager, or name the project on the call if it \
             genuinely belongs there.\n{}",
            elsewhere.name,
            crate::ai::persona::format_catalog(known)
        ));
    }
    Err(format!(
        "error: no agent named {requested:?}.\n{}",
        crate::ai::persona::format_catalog(known)
    ))
}

/// Everyone addressable right now: this project's team plus the company-wide agents.
fn team(ctx: &ToolContext) -> Vec<crate::storage::models::Persona> {
    let all = ctx.db.list_personas().unwrap_or_default();
    let here = ctx.workspace_id.as_deref().filter(|s| !s.is_empty());
    crate::ai::persona::team_for(&all, here)
        .into_iter()
        .cloned()
        .collect()
}

fn agent_list(ctx: &ToolContext) -> String {
    let list = team(ctx);
    let scope = match ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) {
        Some(ws) => ctx
            .db
            .get_workspace(ws)
            .ok()
            .flatten()
            .map(|w| format!(" on {}", w.name))
            .unwrap_or_default(),
        None => String::new(),
    };
    format!(
        "Agents you can delegate to{scope} (this project's team, plus company-wide \
         agents):\n{}\n\nAgents on other projects are deliberately not listed — open \
         that project to reach them, or ask their manager.",
        crate::ai::persona::format_catalog(&list)
    )
}

fn agent_delegate(ctx: &ToolContext, args: &Value) -> String {
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("").trim();
    if task.is_empty() {
        return "error: missing 'task'".into();
    }
    let requested = args.get("agent").and_then(|v| v.as_str()).unwrap_or("").trim();

    // Which project this task belongs to. Naming one is how the single conversation the
    // user has reaches a team whose project is not the one open in front of them.
    let project = match args.get("project").and_then(|v| v.as_str()).map(str::trim) {
        Some(name) if !name.is_empty() => {
            let all = ctx.db.list_workspaces().unwrap_or_default();
            match all
                .iter()
                .find(|w| w.name.eq_ignore_ascii_case(name) || w.id == name)
            {
                Some(w) => Some(w.id.clone()),
                None => {
                    return format!(
                        "error: no project called {name:?}. Known projects: {}",
                        all.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
                    )
                }
            }
        }
        _ => ctx.workspace_id.clone().filter(|s| !s.is_empty()),
    };

    let all = ctx.db.list_personas().unwrap_or_default();
    let known: Vec<crate::storage::models::Persona> =
        crate::ai::persona::team_for(&all, project.as_deref())
            .into_iter()
            .cloned()
            .collect();

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
        // Only somebody on this project's team, or a company-wide agent. Naming an
        // agent used to search the whole database, so one project's lead could hand
        // work to another project's engineer and nothing said a word.
        let scoped = project.is_some();
        match find_addressable(&known, &all, requested, scoped) {
            Ok(p) => (p, false),
            Err(e) => return e,
        }
    };
    if !persona.enabled {
        return format!("error: agent {} is disabled; enable it in Settings → Agents first", persona.name);
    }
    // An unscoped call (the agent the user talks to, with no project open) that names
    // somebody files the work under *their* project rather than nowhere. Otherwise a
    // task handed out over chat lands in a global pool and its own team never sees it.
    let project = project.or_else(|| persona.workspace_id.clone());

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

    // None unless the caller asked for a budget.
    let max_cycles: Option<i64> = args
        .get("max_cycles")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0);

    let id = match start_persona_task(
        &ctx.app,
        &ctx.db,
        &persona,
        task,
        success_criteria,
        targets.clone(),
        project.clone(),
        max_cycles,
    ) {
        Ok(id) => id,
        Err(e) => return format!("error creating delegated task: {e}"),
    };

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
        "Delegated to {name}{how} (task_id {id}, {where_}{budget}).\n\
         {name} is working on it in the background now and the user is notified when it \
         finishes. Do not wait for it — carry on with what the user asked. Use \
         agent_check(task_id: \"{id}\") to see progress.",
        name = persona.name,
        how = how,
        budget = max_cycles
            .map(|n| format!(", capped at {n} cycles"))
            .unwrap_or_default(),
    )
}

/// Start a goal as this persona and drive it. Used by delegation, inbox-wake, and
/// standing duty — one insert path so a duty cannot skip the project stamp or the
/// verify-before-done check that assigned work already has.
pub fn start_persona_task(
    app: &AppHandle,
    db: &Db,
    persona: &Persona,
    task: &str,
    success_criteria: Vec<String>,
    targets: Vec<String>,
    workspace_id: Option<String>,
    max_cycles: Option<i64>,
) -> Result<String, String> {
    let spec = GoalSpec {
        objective: task.to_string(),
        success_criteria,
        check_method: "Verify with tools against the servers before claiming done. \
                       goal_check_criteria(met) is refused when nothing was recorded."
            .into(),
        check_tooling: vec![],
        hard_constraints: vec![
            "Do not claim done without tool output, a file change, or a kanban note \
             that cites what you actually ran."
                .into(),
            "Do not edit the shared default branch. repo_start a wip/<you>/<task> \
             worktree first; tell the team the branch and files; repo_finish when \
             done so the branch is deleted."
                .into(),
        ],
        max_cycles,
        vps_targets: targets,
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
        workspace_id,
        outcome: None,
        request_id: None,
        reported_at: None,
        pr_number: None,
        approval_state: None,
    };
    db.insert_goal(&goal).map_err(|e| e.to_string())?;
    crate::ai::goal::spawn_from_app(app, &id);
    Ok(id)
}

/// The prompt an idle agent wakes up to. Named as a task because that is all a
/// goal loop is: it does not remember being idle, so the reason it is awake has
/// to be in front of it.
pub fn duty_task(persona: &Persona) -> String {
    let remit = if persona.role.trim().is_empty() {
        "your standing role"
    } else {
        persona.role.trim()
    };
    format!(
        "Standing work on your remit\n\n\
         {DUTY_MARK} Nobody assigned you a task. That is not permission to sit idle. \
         You are {}, {remit}.\n\
         1. Read agent_inbox first. If there is work, do it.\n\
         2. Otherwise do the next useful piece of work in your remit, on your servers, \
            with tools — not a plan, the work.\n\
         3. Verify every claim with tool output before you report it. A 'done' with no \
            file change and no kanban note citing what you ran will be refused.\n\
         4. Claude Code CLI is disabled. Do the work yourself (run_command, files, git, \
            kubectl, docker).\n\
         5. One verified result this run, then agent_report and stop. If you checked \
            and nothing needs doing, say so with the commands you ran and stop.\n\
         Do not pad the cycle with plans. Do not invent status.\n\
         Do not trample a teammate: repo_status first, join their wip/ if they already \
         cover the files, otherwise repo_start your own worktree.",
        persona.name
    )
}

/// Drive an existing open task, or start a standing-duty run, so a message is not
/// a note in a drawer nobody opens. An "active" goal whose loop died with the
/// process is restarted rather than stacked with a second task.
pub fn wake_persona(app: &AppHandle, db: &Db, persona_id: &str) {
    let Some(p) = crate::ai::persona::resolve(db, persona_id) else {
        return;
    };
    if !p.enabled {
        return;
    }
    let goals = db.list_goals().unwrap_or_default();
    if let Some(g) = goals.iter().find(|g| {
        g.persona_id.as_deref() == Some(p.id.as_str())
            && (g.status == "active" || g.status == "waiting")
    }) {
        if g.status == "waiting" {
            let mut g = g.clone();
            g.status = "active".into();
            g.next_check_at = None;
            if let Err(e) = db.update_goal(&g) {
                crate::diag(&format!("wake {}: could not resume waiting task: {e}", p.name));
                return;
            }
        }
        crate::ai::goal::spawn_from_app(app, &g.id);
        return;
    }
    let targets = p.targets.clone();
    let ws = p.workspace_id.clone();
    if let Err(e) = start_persona_task(
        app,
        db,
        &p,
        &duty_task(&p),
        vec![
            "Did real work in your remit, verified with tool output".into(),
            "OR confirmed with commands that nothing needs doing right now".into(),
        ],
        targets,
        ws,
        None,
    ) {
        crate::diag(&format!("wake {}: could not start standing work: {e}", p.name));
    }
}

pub fn parse_goal_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        })
}

/// Who should get a standing-duty run this tick: enabled, not already on a task,
/// either holding unread mail or past the cooldown, oldest-idle first, capped.
pub fn idle_duty_picks(
    personas: &[Persona],
    goals: &[GoalSession],
    unread: &HashSet<String>,
    now: DateTime<Utc>,
    cooldown: Duration,
    cap: usize,
) -> Vec<String> {
    if cap == 0 {
        return Vec::new();
    }
    let mut busy: HashSet<String> = HashSet::new();
    let mut last: HashMap<String, DateTime<Utc>> = HashMap::new();
    for g in goals {
        let Some(pid) = g.persona_id.as_deref() else {
            continue;
        };
        if g.status == "active" || g.status == "waiting" {
            busy.insert(pid.to_string());
        }
        for ts in [
            g.finished_at.as_deref(),
            g.updated_at.as_deref(),
            g.created_at.as_deref(),
        ] {
            if let Some(t) = ts.and_then(parse_goal_ts) {
                last.entry(pid.to_string())
                    .and_modify(|e| {
                        if t > *e {
                            *e = t;
                        }
                    })
                    .or_insert(t);
            }
        }
    }
    let mut cands: Vec<(String, DateTime<Utc>)> = personas
        .iter()
        .filter(|p| p.enabled && !busy.contains(&p.id))
        .filter(|p| {
            if unread.contains(&p.id) {
                return true;
            }
            match last.get(&p.id) {
                Some(t) => now.signed_duration_since(*t) >= cooldown,
                None => true,
            }
        })
        .map(|p| {
            (
                p.id.clone(),
                last.get(&p.id)
                    .copied()
                    .unwrap_or(DateTime::<Utc>::MIN_UTC),
            )
        })
        .collect();
    cands.sort_by_key(|(_, t)| *t);
    cands.into_iter().take(cap).map(|(id, _)| id).collect()
}

/// What the machine recorded for a run, not what the agent wrote in the report.
#[derive(Debug, Clone)]
pub struct WorkRecord {
    pub evidence: String,
    pub file_changes: usize,
    pub kanban_notes: usize,
    pub commands: Option<usize>,
}

/// Notes the agent put on the board. Empty cards do not count — "in_progress" with
/// no result is a label, not a finding.
pub fn kanban_note_count(kanban_json: &str) -> usize {
    let tasks: Vec<Value> = serde_json::from_str(kanban_json).unwrap_or_default();
    tasks
        .iter()
        .filter(|t| {
            let result = t
                .get("result")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            let history = t
                .get("history")
                .and_then(|v| v.as_array())
                .map(|h| {
                    h.iter().any(|ev| {
                        ev.get("note")
                            .and_then(|v| v.as_str())
                            .map(|s| !s.trim().is_empty())
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            result || history
        })
        .count()
}

/// Combinations that cannot both be true: a "done" with nothing behind it, or a
/// test/deploy claim with no commands and no files. Used by task_audit (after the
/// fact) and by goal_check_criteria (so the lie never lands).
pub fn done_contradictions(rec: &WorkRecord) -> Vec<String> {
    let mut flags: Vec<String> = Vec::new();
    if rec.evidence.trim().is_empty() {
        flags.push(
            "no evidence was given. A 'done' without what you ran or observed is a \
             claim, not a result."
                .into(),
        );
    }
    let no_files = rec.file_changes == 0;
    let no_notes = rec.kanban_notes == 0;
    // commands == None means the transcript was cleaned up, not that nothing ran.
    // Still refuse when the board and the edit journal are empty too — that
    // combination is "I say I did it" with nowhere to look.
    if no_files && no_notes && rec.commands.is_none() {
        flags.push(
            "nothing was recorded: no file change, no kanban note, and no transcript \
             of commands. Either the work was already done — in which case say that, \
             with what you checked — or it was not done."
                .into(),
        );
    }
    if no_files && no_notes && rec.commands == Some(0) {
        flags.push(
            "reported done, but nothing was changed, no command was run, and the \
             board has no notes. Either the work was already done — in which case \
             the report should say so, with what you checked — or it was not done."
                .into(),
        );
    }
    let claim_lower = rec.evidence.to_lowercase();
    if claim_lower.contains("test") && rec.commands == Some(0) && no_files {
        flags.push("claims testing, but the session ran no commands and changed no files.".into());
    }
    if (claim_lower.contains("deploy") || claim_lower.contains("push"))
        && no_files
        && no_notes
        && rec.commands == Some(0)
    {
        flags.push("claims a deploy or push with nothing recorded behind it.".into());
    }
    flags
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
pub(crate) fn current_persona(ctx: &ToolContext) -> Option<crate::storage::models::Persona> {
    // Set directly when the turn *is* an agent without being a goal — a message
    // arriving over remote chat, answered by the agent the user put in charge.
    if let Some(id) = ctx.persona_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(p) = ctx.db.get_persona(id).ok().flatten() {
            return Some(p);
        }
    }
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
        channel_id: None,
        parent_id: None,
        mentions: Vec::new(),
        resolved_at: None,
    };
    ctx.db
        .insert_agent_message(&msg)
        .map_err(|e| e.to_string())?;
    // The UI mirrors the exchange live, so the user can watch the agents talk rather
    // than discovering afterwards that they did.
    let _ = ctx.app.emit("agent://message", &msg);
    // A message nobody is running to read is a message that never arrived. Wake the
    // recipient so they actually do the work, instead of waiting for a cycle that
    // will not start because they are idle.
    if let Some(to) = to_id.as_deref().filter(|s| !s.is_empty()) {
        wake_persona(&ctx.app, &ctx.db, to);
    }
    Ok(())
}

fn agent_send(ctx: &ToolContext, args: &Value) -> String {
    let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("").trim();
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
    if to.is_empty() || body.is_empty() {
        return "error: agent_send needs both 'to' and 'body'".into();
    }
    // Same rule as delegation: you may write to your own project's team and to the
    // company-wide agents. A message is work, so reaching another project by name was
    // the same hole in a different tool.
    let all = ctx.db.list_personas().unwrap_or_default();
    let known = team(ctx);
    let scoped = ctx.workspace_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    let recipient = match find_addressable(&known, &all, to, scoped) {
        Ok(p) => p,
        Err(e) => return e,
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
        "Message sent from {from_name} to {}. They are being woken to read it — \
         do not wait for a reply, carry on and check agent_inbox later.",
        recipient.name
    )
}

// ---------------------------------------------------------------------------
// The approval ladder
// ---------------------------------------------------------------------------

/// Mark the goal this turn belongs to as waiting on a decision, or released from one.
///
/// The tool dispatcher reads this field, not the proposal table: a guard that has to
/// join two tables on every tool call is a guard somebody turns off.
fn set_approval_state(ctx: &ToolContext, state: Option<&str>) {
    let Some(goal_id) = ctx
        .goal_id
        .clone()
        .or_else(|| ctx.session_id.strip_prefix("goal:").map(str::to_string))
    else {
        return;
    };
    if let Ok(Some(mut goal)) = ctx.db.get_goal(&goal_id) {
        goal.approval_state = state.map(str::to_string);
        if let Err(e) = ctx.db.update_goal(&goal) {
            crate::diag(&format!("proposal: could not mark goal {goal_id}: {e}"));
        }
    }
}

async fn feature_propose(ctx: &ToolContext, args: &Value) -> String {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
    if title.is_empty() || body.is_empty() {
        return "error: feature_propose needs both 'title' and 'body'".into();
    }
    let Some(me) = current_persona(ctx) else {
        // The main agent is already talking to the user. Asking them is one sentence,
        // and routing it through a table would only delay the answer.
        return "You are talking to the user directly — ask them whether to build it \
                rather than filing a proposal."
            .into();
    };
    let goal_id = ctx
        .goal_id
        .clone()
        .or_else(|| ctx.session_id.strip_prefix("goal:").map(str::to_string));
    let proposal = crate::storage::models::FeatureProposal {
        id: uuid::Uuid::new_v4().to_string(),
        workspace_id: ctx.workspace_id.clone().filter(|s| !s.is_empty()),
        persona_id: Some(me.id.clone()),
        goal_id,
        title: title.to_string(),
        body: body.to_string(),
        state: "proposed".into(),
        decided_by: None,
        decision_note: None,
        created_at: None,
        updated_at: None,
    };
    if let Err(e) = ctx.db.insert_feature_proposal(&proposal) {
        return format!("error filing the proposal: {e}");
    }
    // Held from here, so an agent cannot start building while its own proposal is
    // still climbing the chain.
    set_approval_state(ctx, Some("proposed"));

    let outcome = crate::ai::escalation::review_proposal(&ctx.db, &me, None, title, body).await;
    let decided_by = outcome.decided_by.as_ref().map(|p| p.id.clone());
    if outcome.approved {
        let _ = ctx.db.decide_feature_proposal(
            &proposal.id,
            "approved",
            decided_by.as_deref(),
            Some(&outcome.summary()),
        );
        set_approval_state(ctx, Some("approved"));
        return format!(
            "Approved.\n{}\n\nBuild it. Keep it to what you proposed — anything beyond \
             that is a new proposal.",
            outcome.summary()
        );
    }
    if outcome.refused {
        let _ = ctx.db.decide_feature_proposal(
            &proposal.id,
            "rejected",
            decided_by.as_deref(),
            Some(&outcome.summary()),
        );
        set_approval_state(ctx, Some("rejected"));
        return format!(
            "Not approved.\n{}\n\nDo not build it. Carry on with the work you already \
             have, or report back if that was all of it.",
            outcome.summary()
        );
    }
    // Nobody answered. It stays open and a person decides — which is why it is also
    // reported upward rather than left in a table nobody opens.
    let all = ctx.db.list_personas().unwrap_or_default();
    let to = crate::ai::persona::manager_of(&all, &me).map(|m| m.id.clone());
    let note = format!("Proposal ({}): {title}\n\n{body}", proposal.id);
    if to.is_none() {
        crate::ai::report::report_to_user(&ctx.db, ctx.goal_id.as_deref(), &me.name, &note);
    }
    let _ = record_message(ctx, Some(me.id.clone()), to, "report", &note);
    format!(
        "Filed as proposal {} and not approved yet.\n{}\n\nDo not build it while it is \
         open. Work on something already agreed, and check back with feature_list.",
        proposal.id,
        outcome.summary()
    )
}

fn feature_decide(ctx: &ToolContext, args: &Value) -> String {
    let id = args.get("proposal_id").and_then(|v| v.as_str()).unwrap_or("").trim();
    let decision = args
        .get("decision")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").trim();
    if id.is_empty() {
        return "error: feature_decide needs a 'proposal_id' — feature_list has them".into();
    }
    let state = match decision.as_str() {
        "approve" | "approved" | "yes" => "approved",
        "reject" | "rejected" | "no" => "rejected",
        _ => return "error: decision must be \"approve\" or \"reject\"".into(),
    };
    let proposal = match ctx.db.get_feature_proposal(id) {
        Ok(Some(p)) => p,
        Ok(None) => return format!("error: no proposal {id}"),
        Err(e) => return format!("error reading the proposal: {e}"),
    };
    if !task_visible(proposal.workspace_id.as_deref(), ctx.workspace_id.as_deref()) {
        return "error: that proposal belongs to another project. Its own chain of \
                command decides it."
            .into();
    }
    let me = current_persona(ctx);
    let by = me.as_ref().map(|p| p.id.clone());
    match ctx
        .db
        .decide_feature_proposal(id, state, by.as_deref(), (!note.is_empty()).then_some(note))
    {
        Ok(false) => return format!("error: no proposal {id}"),
        Err(e) => return format!("error recording the decision: {e}"),
        Ok(true) => {}
    }
    // The agent that asked is held by its goal row, not by the proposal, so releasing
    // it is a separate write — and the one that actually lets work resume.
    if let Some(goal_id) = proposal.goal_id.as_deref() {
        if let Ok(Some(mut goal)) = ctx.db.get_goal(goal_id) {
            goal.approval_state = Some(state.to_string());
            let _ = ctx.db.update_goal(&goal);
        }
    }
    if let Some(to) = proposal.persona_id.clone() {
        let body = format!(
            "Your proposal \"{}\" was {state}.{}",
            proposal.title,
            if note.is_empty() { String::new() } else { format!(" {note}") }
        );
        let _ = record_message(ctx, by, Some(to), "report", &body);
    }
    format!(
        "\"{}\" is {state}. {} has been told.",
        proposal.title,
        display_name(ctx, proposal.persona_id.as_deref())
    )
}

fn feature_list(ctx: &ToolContext, args: &Value) -> String {
    let state = args
        .get("state")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);
    let project = match args.get("project").and_then(|v| v.as_str()).map(str::trim) {
        Some(n) if !n.is_empty() => {
            let all = ctx.db.list_workspaces().unwrap_or_default();
            match all.iter().find(|w| w.name.eq_ignore_ascii_case(n) || w.id == n) {
                Some(w) => Some(w.id.clone()),
                None => return format!("error: no project called {n:?}"),
            }
        }
        _ => ctx.workspace_id.clone().filter(|s| !s.is_empty()),
    };
    let rows = match ctx
        .db
        .list_feature_proposals(project.as_deref(), state.or(Some("proposed")), limit)
    {
        Ok(r) => r,
        Err(e) => return format!("error reading proposals: {e}"),
    };
    if rows.is_empty() {
        return "Nothing proposed.".into();
    }
    let lines: Vec<String> = rows
        .iter()
        .map(|p| {
            format!(
                "- [{}] {} — {} ({})\n  {}",
                p.state,
                p.title,
                display_name(ctx, p.persona_id.as_deref()),
                p.id,
                p.body.lines().next().unwrap_or("").chars().take(160).collect::<String>()
            )
        })
        .collect();
    format!("Proposals:\n{}", lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Task control
// ---------------------------------------------------------------------------

/// Whether a task on `goal_ws` may be controlled from a turn working on `here`.
///
/// A task belongs to a project, and so does the authority to stop it. A turn with no
/// project — the agent the user talks to — sees all of them, which is the whole reason
/// intervening from a phone works at all.
pub(crate) fn task_visible(goal_ws: Option<&str>, here: Option<&str>) -> bool {
    match here.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(here) => goal_ws.map(str::trim).filter(|s| !s.is_empty()) == Some(here),
    }
}

/// The task a control tool is about, refused when it is not this turn's to touch.
fn task_in_scope(ctx: &ToolContext, args: &Value) -> Result<GoalSession, String> {
    let id = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("needs a 'task_id' — get one from agent_check or teams_overview")?;
    let goal = ctx
        .db
        .get_goal(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no task {id}"))?;
    if !task_visible(goal.workspace_id.as_deref(), ctx.workspace_id.as_deref()) {
        let whose = goal
            .workspace_id
            .as_deref()
            .and_then(|w| ctx.db.get_workspace(w).ok().flatten())
            .map(|w| w.name)
            .unwrap_or_else(|| "another project".into());
        return Err(format!(
            "that task belongs to {whose}, not to the project you are working on. Its own \
             team runs it — tell them, or open that project."
        ));
    }
    Ok(goal)
}

/// Who is running a task, for the line the caller reads back.
fn owner_of(ctx: &ToolContext, goal: &GoalSession) -> String {
    goal.persona_id
        .as_deref()
        .map(|id| display_name(ctx, Some(id)))
        .unwrap_or_else(|| "the main agent".into())
}

fn task_stop(ctx: &ToolContext, args: &Value) -> String {
    let goal = match task_in_scope(ctx, args) {
        Ok(g) => g,
        Err(e) => return format!("error: {e}"),
    };
    let who = owner_of(ctx, &goal);
    let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("").trim();
    match crate::commands::goal::stop_goal_inner(&ctx.app, &ctx.db, &ctx.session_state, &goal.id) {
        Ok(_) => format!(
            "Stopped \"{}\" ({}). {who} is no longer working on it and the board is kept.{}",
            goal.title,
            goal.id,
            if reason.is_empty() { String::new() } else { format!(" Reason: {reason}") }
        ),
        Err(e) => format!("error stopping the task: {e}"),
    }
}

fn task_pause(ctx: &ToolContext, args: &Value) -> String {
    let goal = match task_in_scope(ctx, args) {
        Ok(g) => g,
        Err(e) => return format!("error: {e}"),
    };
    let who = owner_of(ctx, &goal);
    let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("").trim();
    match crate::commands::goal::pause_goal_inner(&ctx.app, &ctx.db, &ctx.session_state, &goal.id) {
        Ok(_) => format!(
            "Paused \"{}\" ({}). {who} stops where it is; task_resume picks it back up.{}",
            goal.title,
            goal.id,
            if reason.is_empty() { String::new() } else { format!(" Reason: {reason}") }
        ),
        Err(e) => format!("error pausing the task: {e}"),
    }
}

fn task_resume(ctx: &ToolContext, args: &Value) -> String {
    let goal = match task_in_scope(ctx, args) {
        Ok(g) => g,
        Err(e) => return format!("error: {e}"),
    };
    let who = owner_of(ctx, &goal);
    let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").trim();
    // Said before it starts, so the agent reads it on its first cycle rather than
    // discovering later that somebody restarted it for a reason.
    if !note.is_empty() {
        if let Some(to) = goal.persona_id.clone() {
            let me = current_persona(ctx).map(|p| p.id);
            let _ = record_message(ctx, me, Some(to), "note", note);
        }
    }
    match crate::commands::goal::resume_goal_inner(&ctx.app, &ctx.db, &ctx.session_state, &goal.id) {
        Ok(_) => format!(
            "Resumed \"{}\" ({}). {who} is working on it again — do not wait for it.",
            goal.title, goal.id
        ),
        Err(e) => format!("error resuming the task: {e}"),
    }
}

fn task_reassign(ctx: &ToolContext, args: &Value) -> String {
    let mut goal = match task_in_scope(ctx, args) {
        Ok(g) => g,
        Err(e) => return format!("error: {e}"),
    };
    let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("").trim();
    if to.is_empty() {
        return "error: task_reassign needs 'to' — the agent taking it over".into();
    }
    let all = ctx.db.list_personas().unwrap_or_default();
    // The task's project decides who may take it, not the turn's: a task filed under a
    // project stays with that project's team even when an unscoped turn is moving it.
    let known: Vec<Persona> =
        crate::ai::persona::team_for(&all, goal.workspace_id.as_deref().filter(|s| !s.is_empty()))
            .into_iter()
            .cloned()
            .collect();
    let scoped = goal.workspace_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    let next = match find_addressable(&known, &all, to, scoped) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !next.enabled {
        return format!("error: {} is disabled, so it cannot take work", next.name);
    }
    let previous = owner_of(ctx, &goal);
    if goal.persona_id.as_deref() == Some(next.id.as_str()) {
        return format!("{} already owns that task.", next.name);
    }

    let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").trim();
    goal.persona_id = Some(next.id.clone());
    if let Err(e) = ctx.db.update_goal(&goal) {
        return format!("error reassigning the task: {e}");
    }
    // The handover note reaches the new owner as a message, which also wakes them —
    // otherwise a reassigned task sits still until something else happens to run.
    let handover = if note.is_empty() {
        format!("You now own \"{}\" (task {}), taken over from {previous}.", goal.title, goal.id)
    } else {
        format!(
            "You now own \"{}\" (task {}), taken over from {previous}. {note}",
            goal.title, goal.id
        )
    };
    let me = current_persona(ctx).map(|p| p.id);
    if let Err(e) = record_message(ctx, me, Some(next.id.clone()), "request", &handover) {
        return format!("reassigned, but the handover note did not send: {e}");
    }
    format!(
        "\"{}\" ({}) is now {}'s, taken from {previous}. They have the handover note and \
         are being woken to pick it up.",
        goal.title, goal.id, next.name
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
        // No manager: this report is for the user. `goal://notify` reached no frontend
        // file at all, so a top-of-chain agent's report ended here silently. The outbox
        // is the path that actually delivers it, to the chat that asked.
        crate::ai::report::report_to_user(&ctx.db, ctx.goal_id.as_deref(), &me.name, body);
    }
    format!("Reported to {to_name}.")
}

fn agent_inbox(ctx: &ToolContext) -> String {
    let me = current_persona(ctx);
    let my_id = me.as_ref().map(|p| p.id.clone());
    let msgs = match ctx.db.unread_agent_messages(my_id.as_deref()) {
        Ok(m) => m,
        Err(e) => return format!("error reading inbox: {e}"),
    };
    if msgs.is_empty() {
        return "No new messages.".into();
    }

    // Every message says which project it is about. Filtering by project instead was
    // the wrong fix for the right complaint: the problem was never that other projects'
    // messages arrived, it was not knowing which was which — and filtering meant a lead
    // writing to another project's lead was never heard at all.
    let here = ctx.workspace_id.as_deref().filter(|s| !s.is_empty());
    let project_of = |id: Option<&str>| -> String {
        match id {
            Some(id) if Some(id) == here => String::new(),
            Some(id) => ctx
                .db
                .get_workspace(id)
                .ok()
                .flatten()
                .map(|w| format!(" (about {})", w.name))
                .unwrap_or_default(),
            None => String::new(),
        }
    };

    let lines: Vec<String> = msgs
        .iter()
        .map(|m| {
            format!(
                "[{}] from {}{}: {}",
                m.kind,
                display_name(ctx, m.from_id.as_deref()),
                project_of(m.workspace_id.as_deref()),
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
    format!(
        "{} new message(s):\n{}\n\nAnything marked \"about <project>\" concerns a \
         different project from the one you are working on — answer it, but do the work \
         in that project's context, not this one.",
        msgs.len(),
        lines.join("\n")
    )
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
    let list_arg = |k: &str| {
        args.get(k).and_then(|v| v.as_array()).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>()
        })
    };

    // Renaming is a hire against the old name. Refusing to invent an agent under the new
    // one matters: a typo would otherwise quietly create a second agent and leave the
    // first where it was.
    let rename_to = str_arg("rename_to").filter(|s| !s.is_empty());
    if rename_to.is_some() && existing.is_none() {
        return format!("error: there is no agent named {name:?} to rename.");
    }
    let final_name = rename_to.unwrap_or(name);

    // Which project this agent belongs to. Over chat there is no project open, so every
    // hire used to land company-wide — an agent addressable from every project and
    // filed under none of them, which is the opposite of what "add a backend engineer
    // to CSB" means.
    let workspace_id = match str_arg("project") {
        Some(p) if matches!(p.to_lowercase().as_str(), "company-wide" | "companywide" | "none" | "all" | "any") => None,
        Some(p) if !p.is_empty() => {
            let all = ctx.db.list_workspaces().unwrap_or_default();
            match all.iter().find(|w| w.name.eq_ignore_ascii_case(p) || w.id == p) {
                Some(w) => Some(w.id.clone()),
                None => {
                    return format!(
                        "error: no project called {p:?}. Known projects: {}",
                        all.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
                    )
                }
            }
        }
        // Unstated: keep what it has, else the project being worked on. Company-wide is
        // now something you ask for rather than something you fall into.
        _ => existing
            .as_ref()
            .and_then(|p| p.workspace_id.clone())
            .or_else(|| ctx.workspace_id.clone().filter(|s| !s.is_empty())),
    };

    let provider_id = match str_arg("provider_id").filter(|s| !s.is_empty()) {
        Some(want) => {
            let providers = ctx.db.list_providers().unwrap_or_default();
            match providers
                .iter()
                .find(|p| p.id == want || p.name.eq_ignore_ascii_case(want))
            {
                Some(p) => Some(p.id.clone()),
                None => {
                    return format!(
                        "error: no provider called {want:?}. Configured: {}",
                        providers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
                    )
                }
            }
        }
        None => existing.as_ref().and_then(|p| p.provider_id.clone()),
    };

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
        name: final_name.to_string(),
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
        provider_id,
        model: str_arg("model")
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| existing.as_ref().and_then(|p| p.model.clone())),
        enabled: args
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| existing.as_ref().map(|p| p.enabled).unwrap_or(true)),
        reports_to,
        workspace_id,
        allowed_paths: list_arg("allowed_paths")
            .or_else(|| existing.as_ref().map(|p| p.allowed_paths.clone()))
            .unwrap_or_default(),
        allowed_tools: list_arg("allowed_tools")
            .or_else(|| existing.as_ref().map(|p| p.allowed_tools.clone()))
            .unwrap_or_default(),
    };

    let manager_name = input
        .reports_to
        .as_deref()
        .map(|id| display_name(ctx, Some(id)))
        .unwrap_or_else(|| "the user".into());
    let project_name = match input.workspace_id.as_deref() {
        Some(id) => ctx
            .db
            .get_workspace(id)
            .ok()
            .flatten()
            .map(|w| w.name)
            .unwrap_or_else(|| "(deleted project)".into()),
        None => "company-wide — reachable from every project".into(),
    };
    let renamed = existing
        .as_ref()
        .filter(|p| p.name != input.name)
        .map(|p| format!(" (renamed from \"{}\")", p.name))
        .unwrap_or_default();
    let summary = format!(
        "{} the agent \"{}\"{renamed}.\n\
         Role: {}\n\
         Project: {}\n\
         Reports to: {}\n\
         Servers: [{}]   Trust: {}   {}\n\
         Model: {}\n\
         Files: {}\n\
         Tools: {}",
        if existing.is_some() { "Update" } else { "Create" },
        input.name,
        if input.role.is_empty() { "(none given)" } else { &input.role },
        project_name,
        manager_name,
        input.targets.join(", "),
        input.safety_mode.clone().unwrap_or_else(|| "global default".into()),
        if input.enabled { "May be given work." } else { "Disabled." },
        input.model.clone().unwrap_or_else(|| "provider default".into()),
        if input.allowed_paths.is_empty() {
            "the whole project".to_string()
        } else {
            input.allowed_paths.join(", ")
        },
        if input.allowed_tools.is_empty() {
            "every tool".to_string()
        } else {
            input.allowed_tools.join(", ")
        },
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

/// One agent's whole configuration, so a change is made with what is there in view.
fn agent_inspect(ctx: &ToolContext, args: &Value) -> String {
    let want = args.get("agent").and_then(|v| v.as_str()).unwrap_or("").trim();
    if want.is_empty() {
        return "error: agent_inspect needs an 'agent'".into();
    }
    let all = ctx.db.list_personas().unwrap_or_default();
    let known = team(ctx);
    let scoped = ctx.workspace_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    let p = match find_addressable(&known, &all, want, scoped) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let project = match p.workspace_id.as_deref() {
        Some(id) => ctx
            .db
            .get_workspace(id)
            .ok()
            .flatten()
            .map(|w| w.name)
            .unwrap_or_else(|| "(deleted project)".into()),
        None => "company-wide".into(),
    };
    let manager = crate::ai::persona::manager_of(&all, &p)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "the user".into());
    let provider = p
        .provider_id
        .as_deref()
        .and_then(|id| ctx.db.get_provider(id).ok().flatten())
        .map(|pr| pr.name)
        .unwrap_or_else(|| "the active provider".into());
    let running: Vec<String> = ctx
        .db
        .list_goals()
        .unwrap_or_default()
        .into_iter()
        .filter(|g| {
            g.persona_id.as_deref() == Some(p.id.as_str())
                && matches!(g.status.as_str(), "active" | "waiting" | "paused" | "blocked")
        })
        .map(|g| format!("  - [{}] {} ({})", g.status, g.title, g.id))
        .collect();

    format!(
        "{name} ({id})\n\
         Role: {role}\n\
         Project: {project}\n\
         Reports to: {manager}\n\
         Servers: {targets}\n\
         Trust: {trust}\n\
         Provider: {provider}   Model: {model}\n\
         Files it may touch: {paths}\n\
         Tools it may call: {tools}\n\
         {state}\n\
         Open tasks:\n{running}\n\
         Standing instructions:\n{instructions}",
        name = p.name,
        id = p.id,
        role = if p.role.trim().is_empty() { "(none set)" } else { p.role.trim() },
        project = project,
        manager = manager,
        targets = if p.targets.is_empty() { "none".to_string() } else { p.targets.join(", ") },
        trust = p.safety_mode.clone().unwrap_or_else(|| "the global default".into()),
        provider = provider,
        model = p.model.clone().unwrap_or_else(|| "the provider default".into()),
        paths = if p.allowed_paths.is_empty() {
            "the whole project".to_string()
        } else {
            p.allowed_paths.join(", ")
        },
        tools = if p.allowed_tools.is_empty() {
            "every tool".to_string()
        } else {
            p.allowed_tools.join(", ")
        },
        state = if p.enabled { "Active — may be given work." } else { "Disabled." },
        running = if running.is_empty() { "  (none)".to_string() } else { running.join("\n") },
        instructions = if p.instructions.trim().is_empty() {
            "  (none)".to_string()
        } else {
            p.instructions.trim().to_string()
        },
    )
}

/// What one default role is for, how far it is trusted, and what it may touch.
///
/// A struct rather than a tuple because it grew a fourth and fifth field and a
/// five-tuple is unreadable at the call site.
pub struct RoleDefaults {
    /// The one-line remit. Work is routed by matching against this, so it describes the
    /// work rather than the personality.
    pub blurb: &'static str,
    pub instructions: &'static str,
    /// Safety mode. `None` uses the global default.
    pub trust: Option<&'static str>,
    /// Globs inside the project this role may change. Empty = the whole project.
    pub paths: &'static [&'static str],
    /// Tools it may call. Empty = every tool.
    pub tools: &'static [&'static str],
}

/// What each default role is for, how much it is trusted, and what it may touch.
///
/// Trust is the part worth getting right up front. A reviewer that can change things is
/// not a reviewer, and an engineer that can do anything unattended is a much bigger
/// decision than "add an engineer" sounds like — so the defaults are narrow and the
/// user widens them deliberately.
///
/// The paths and tools are the same idea, one level down, and they are what make these
/// roles different from each other in fact rather than in prose: a lead that delegates
/// is a lead that cannot write files, and a researcher that reads is a researcher with
/// no write tool at all. Every list here is a default a person can widen in Settings →
/// Agents; none of them is a security boundary against the user.
pub fn role_defaults(role: &str) -> RoleDefaults {
    match role.trim().to_lowercase().as_str() {
        "lead" | "ceo" | "manager" => RoleDefaults {
            blurb: "leads this project and answers to the user",
            instructions: "You lead this project. Route work to your team rather than \
             doing it all yourself, keep an eye on the project's numbers, and report \
             upward: what changed, what you decided, what you need. You do not edit \
             files — that is what your team is for, and doing their work yourself is how \
             two people end up in the same file. Nothing your team does may end with \
             uncommitted work, a leftover wip/ branch, or a pull request left to rot — \
             check repo_status. If somebody proposes something new, decide it: \
             feature_decide, with a reason they can use. Be brief: it is often read on a \
             phone.",
            trust: None,
            // Nothing. A lead that writes files is an engineer with a title.
            paths: &[],
            tools: &[
                "agent_*", "task_*", "team_*", "feature_*", "repo_status", "project_*",
                "read_file", "local_read_file", "local_grep_search", "local_find_files",
                "local_list_dir", "list_dir", "grep_search", "explore", "web_search",
            ],
        },
        "architect" | "principal" | "staff" => RoleDefaults {
            blurb: "decides how this project is built and reviews the shape of changes",
            instructions: "You decide how this project is built. Read widely before you \
             say anything: the shape of a change matters more than its diff. You write \
             documentation and decisions, not implementations — hand those to whoever \
             owns that part of the tree. Say plainly when a proposal duplicates something \
             we already have, and prefer the change that removes a concept to the one \
             that adds one.",
            trust: Some("approve"),
            paths: &["docs/**", "*.md", "**/*.md"],
            tools: &[],
        },
        "backend" | "engineer" | "dev" | "developer" => RoleDefaults {
            blurb: "implements changes to this project's backend",
            instructions: "You implement changes on this project. Read before you write, \
             make the smallest change that does the job, and verify it worked before \
             saying it is done. Reuse what already exists rather than writing a second \
             copy of it. Commit and push before you stop — never leave work in a working \
             tree. If what you are about to build does not exist yet at all, that is a \
             feature_propose, not a commit. Report what you actually changed, not what \
             you intended to.",
            trust: Some("allowlist"),
            paths: &["src-tauri/**"],
            tools: &[],
        },
        "frontend" | "ui" | "web" => RoleDefaults {
            blurb: "implements this project's user interface",
            instructions: "You build the interface. No emojis anywhere in it, icons are \
             SVG components, and the styling stays muted and dark — match what is already \
             there rather than introducing a second look. Read the component next to the \
             one you are changing before you change it. Verify it compiles and the checks \
             pass before saying it is done.",
            trust: Some("allowlist"),
            paths: &["src/**"],
            tools: &[],
        },
        "reviewer" | "qa" | "test" | "deploy" => RoleDefaults {
            blurb: "reviews, tests and verifies this project",
            instructions: "You verify. You read everything and change almost nothing — \
             you check that what was claimed actually happened, and say plainly when it \
             did not. Cite what you looked at. The one thing you do write is tests: a \
             test that would have caught it is worth more than a paragraph about it. \
             Look for the things nobody notices one at a time: logic duplicated instead \
             of reused, code nothing calls any more, a function doing three jobs, and \
             pull requests left open long enough to go stale.",
            trust: Some("approve"),
            // Reads the whole project (the root check still applies); writes tests only.
            paths: &["**/tests/**", "**/*.test.*", "**/*.spec.*", "**/*_test.*", "**/test_*"],
            tools: &[],
        },
        "researcher" | "analyst" | "research" => RoleDefaults {
            blurb: "finds things out for this project without changing anything",
            instructions: "You find things out. You change nothing at all — no files, no \
             servers — and the answer is the whole job. Say where each thing came from, \
             and say when you could not find it rather than filling the gap in. Short \
             beats complete: what was asked, answered.",
            trust: Some("approve"),
            paths: &[],
            tools: &[
                "web_search", "web_fetch", "read_file", "local_read_file",
                "local_grep_search", "local_find_files", "local_list_dir", "list_dir",
                "grep_search", "find_files", "explore", "agent_*", "project_history",
            ],
        },
        "ops" | "sysadmin" | "sre" => RoleDefaults {
            blurb: "keeps this project's servers healthy",
            instructions: "You keep this project's servers healthy: disk, memory, \
             services, backups, certificates. Diagnose before acting, and never restart \
             something without saying why it needed it.",
            trust: Some("allowlist"),
            paths: &[],
            tools: &[],
        },
        _ => RoleDefaults {
            blurb: "works on this project",
            instructions: "You work on this project. Say what you did and what came of it.",
            trust: Some("approve"),
            paths: &[],
            tools: &[],
        },
    }
}

/// "csb" + "lead" -> "CSB Lead". Names are unique across every project, so the project
/// has to be in them: five projects cannot each have an agent called "Lead", and
/// `agent_send "Lead"` would be a coin toss if they could.
pub fn team_member_name(project: &str, role: &str) -> String {
    let short: String = project.split_whitespace().next().unwrap_or(project).chars().take(18).collect();
    let mut role_title = role.trim().to_lowercase();
    if let Some(first) = role_title.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    format!("{} {}", short.trim_end_matches(['.', '-', '_']), role_title)
}

/// The roles a team gets when nobody says otherwise.
///
/// A startup, not a pair: somebody who decides, somebody who decides how, one engineer
/// per side of the tree, and somebody whose job is to disbelieve them. Three was the
/// smallest team that could be said to exist; this is the smallest one that can finish
/// a feature without the user standing in for a missing role.
pub const DEFAULT_ROLES: [&str; 5] = ["lead", "architect", "backend", "frontend", "qa"];

/// One agent a team would gain, before anything is written.
pub struct PlannedMember {
    pub name: String,
    pub blurb: &'static str,
    pub instructions: String,
    pub trust: Option<&'static str>,
    /// What it may change, and what it may call. See [`RoleDefaults`].
    pub paths: Vec<String>,
    pub tools: Vec<String>,
    /// An agent by this name is already there, so it is left alone.
    pub exists: bool,
}

/// Work out the team without creating it, so it can be shown before it is agreed to.
pub fn plan_team(
    db: &crate::storage::Db,
    project_name: &str,
    roles: &[String],
    about: Option<&str>,
) -> Vec<PlannedMember> {
    let existing = db.list_personas().unwrap_or_default();
    roles
        .iter()
        .map(|role| {
            let name = team_member_name(project_name, role);
            let defaults = role_defaults(role);
            let mut instructions = defaults.instructions.to_string();
            if let Some(a) = about.map(str::trim).filter(|a| !a.is_empty()) {
                instructions.push_str(&format!("\n\nThe project: {a}"));
            }
            PlannedMember {
                exists: existing.iter().any(|p| p.name.eq_ignore_ascii_case(&name)),
                name,
                blurb: defaults.blurb,
                instructions,
                trust: defaults.trust,
                paths: defaults.paths.iter().map(|s| s.to_string()).collect(),
                tools: defaults.tools.iter().map(|s| s.to_string()).collect(),
            }
        })
        .collect()
}

/// Create the planned members that do not exist yet. Returns `(created, failed)`.
///
/// The first role is the lead and answers to the user; the rest answer to it, so the
/// lead has to be written first — everyone else needs its id.
pub fn create_team(
    db: &crate::storage::Db,
    workspace_id: &str,
    planned: &[PlannedMember],
) -> (Vec<String>, Vec<String>) {
    let existing = db.list_personas().unwrap_or_default();
    // If the lead was already there, the rest still report to it.
    let mut lead_id = planned.first().filter(|p| p.exists).and_then(|p| {
        existing
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(&p.name))
            .map(|e| e.id.clone())
    });
    let (mut made, mut failed) = (Vec::new(), Vec::new());

    for (i, m) in planned.iter().enumerate() {
        if m.exists {
            continue;
        }
        let is_lead = i == 0;
        let input = crate::storage::models::PersonaInput {
            id: None,
            name: m.name.clone(),
            role: m.blurb.to_string(),
            instructions: m.instructions.clone(),
            targets: Vec::new(),
            safety_mode: m.trust.map(str::to_string),
            provider_id: None,
            model: None,
            enabled: true,
            reports_to: if is_lead { None } else { lead_id.clone() },
            workspace_id: Some(workspace_id.to_string()),
            allowed_paths: m.paths.clone(),
            allowed_tools: m.tools.clone(),
        };
        match crate::commands::persona::save_persona_checked(db, input) {
            Ok(p) => {
                if is_lead {
                    lead_id = Some(p.id.clone());
                }
                made.push(p.name);
            }
            Err(e) => failed.push(format!("{}: {e}", m.name)),
        }
    }
    (made, failed)
}

async fn team_create(ctx: &ToolContext, args: &Value) -> String {
    let all_ws = ctx.db.list_workspaces().unwrap_or_default();
    let named = args.get("project").and_then(|v| v.as_str()).map(str::trim);
    let ws = match named.filter(|n| !n.is_empty()) {
        Some(n) => match all_ws.iter().find(|w| w.name.eq_ignore_ascii_case(n) || w.id == n) {
            Some(w) => w.clone(),
            None => {
                return format!(
                    "error: no project called {n:?}. Known: {}",
                    all_ws.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            }
        },
        None => match ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) {
            Some(id) => match all_ws.iter().find(|w| w.id == id) {
                Some(w) => w.clone(),
                None => return "error: the open project no longer exists".into(),
            },
            None => return "error: no project is open, so name one with `project`".into(),
        },
    };

    let roles: Vec<String> = args
        .get("roles")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::trim))
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_ROLES.iter().map(|r| r.to_string()).collect());
    let about = args.get("about").and_then(|v| v.as_str());

    let planned = plan_team(&ctx.db, &ws.name, &roles, about);
    let to_make: Vec<&PlannedMember> = planned.iter().filter(|p| !p.exists).collect();
    if to_make.is_empty() {
        return format!(
            "{} already has all of those: {}. Nothing to do.",
            ws.name,
            planned.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
        );
    }

    let summary = format!(
        "Create a team for {}:\n{}\n\nThe first answers to you; the rest answer to it. \
         All are limited to {}, and their trust levels start narrow — widen them yourself \
         once you have seen what they do.",
        ws.name,
        to_make
            .iter()
            .map(|m| format!(
                "  - {} — {} [{}] · {}",
                m.name,
                m.blurb,
                m.trust.unwrap_or("global default"),
                if !m.tools.is_empty() {
                    "delegates and reads; no write tools".to_string()
                } else if m.paths.is_empty() {
                    "may change anything in the project".to_string()
                } else {
                    format!("changes {}", m.paths.join(", "))
                }
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        ws.name
    );
    if let Err(e) = confirm(ctx, &summary).await {
        return format!("not created: {e}");
    }

    let (made, failed) = create_team(&ctx.db, &ws.id, &planned);
    let mut out = format!("{} now has: {}.\n", ws.name, made.join(", "));
    for f in &failed {
        out.push_str(&format!("- could not create {f}\n"));
    }
    out.push_str(
        "\nNothing is delegated yet — they have no work until you or a lead gives them \
         some. Consider review_schedule so the project is looked at even when nobody \
         asks.\n",
    );
    out
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

/// Standing instruction for a scheduled review.
///
/// Written as the prompt the agent wakes up to, because that is all a scheduled run is:
/// it does not remember being scheduled, so the reason it is awake has to be in front
/// of it. It names the tool that gathers the briefing rather than repeating the
/// briefing's own structure, which would then have two places to drift.
fn review_prompt(project: &str, focus: Option<&str>) -> String {
    let mut p = format!(
        "This is the recurring review of {project}. Nobody asked for it — it is your \
         standing job to keep this project healthy.\n\n\
         First call metric_collect, so the numbers are today's rather than whenever \
         somebody last looked. Then call project_review to get the briefing: how the numbers moved, what the team \
         did and what came of it, what changed, and what is still open. Then decide, and \
         act:\n\
         - If there are no numbers at all, that is the first thing to fix: set up a \
         source with metric_source_set so they collect themselves. Nothing here is \
         answerable without them.\n\
         - If they fell while the team was busy, say what you think the real cause is and \
         delegate work that would test it — not more of what was already not working.\n\
         - If they rose, say which change you think did it, so it can be repeated.\n\
         - If someone on the team did nothing, either give them work or say their remit \
         is wrong.\n\
         - Before you believe a \"done\" you are going to build on, run task_audit on it. \
         A report is a claim, and an unchecked one gets built on.\n\
         - A stale pull request is not a small thing: left open, the code around it moves \
         until merging it is a rewrite of work already paid for. Get it rebased and \
         merged, or closed with a reason. Never leave it.\n\
         - Leftover wip/<agent>/<task> branches and worktrees are garbage. repo_status, \
         then repo_finish (or delete) anything whose task is done.\n\n\
         Finish with agent_report so it reaches the user: what changed, what you decided, \
         and what you need from them. Keep it short — it is read on a phone."
    );
    if let Some(f) = focus.map(str::trim).filter(|f| !f.is_empty()) {
        p.push_str(&format!("\n\nAlways check this as well: {f}"));
    }
    p
}

async fn review_schedule(ctx: &ToolContext, args: &Value) -> String {
    let all_ws = ctx.db.list_workspaces().unwrap_or_default();
    let named = args.get("project").and_then(|v| v.as_str()).map(str::trim);
    let ws = match named.filter(|n| !n.is_empty()) {
        Some(n) => match all_ws.iter().find(|w| w.name.eq_ignore_ascii_case(n) || w.id == n) {
            Some(w) => w.clone(),
            None => {
                return format!(
                    "error: no project called {n:?}. Known: {}",
                    all_ws.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            }
        },
        None => match ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) {
            Some(id) => match all_ws.iter().find(|w| w.id == id) {
                Some(w) => w.clone(),
                None => return "error: the open project no longer exists".into(),
            },
            None => return "error: no project is open, so name one with `project`".into(),
        },
    };

    let personas = ctx.db.list_personas().unwrap_or_default();
    let runner = match args.get("agent").and_then(|v| v.as_str()).map(str::trim) {
        Some(n) if !n.is_empty() => match crate::ai::persona::resolve(&ctx.db, n) {
            Some(p) => p,
            None => return format!("error: no agent named {n:?}"),
        },
        // Default to that project's lead: the one on it who answers to the user.
        _ => match personas
            .iter()
            .find(|p| {
                p.enabled
                    && p.workspace_id.as_deref() == Some(ws.id.as_str())
                    && p.reports_to.is_none()
            })
            .or_else(|| {
                personas
                    .iter()
                    .find(|p| p.enabled && p.workspace_id.as_deref() == Some(ws.id.as_str()))
            }) {
            Some(p) => p.clone(),
            None => {
                return format!(
                    "error: {} has no agents, so there is nobody to run the review. Create \
                     one with agent_hire first — a review nobody owns is a report nobody \
                     reads.",
                    ws.name
                )
            }
        },
    };

    let schedule = args
        .get("schedule")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("@weekly mon 09:00")
        .to_string();
    if !crate::ai::cron::schedule_is_valid(&schedule) {
        return format!(
            "error: {schedule:?} is not a schedule xConsole understands. Use \"@daily \
             HH:MM\", \"@weekly mon HH:MM\", \"@hourly\", or \"@every 6h\"."
        );
    }
    let enabled = args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let focus = args.get("focus").and_then(|v| v.as_str());

    // One review per project, found by name so calling this twice edits rather than
    // stacking up duplicate reviews that all wake at the same minute.
    let name = format!("Review: {}", ws.name);
    let existing = ctx
        .db
        .list_cron_jobs()
        .unwrap_or_default()
        .into_iter()
        .find(|j| j.name == name);

    let summary = format!(
        "{} the recurring review of {}.\n\
         Runs {schedule}, as {}. It reads the project's numbers and its team's work, \
         decides what to change, and reports back to you.",
        if existing.is_some() { "Update" } else { "Set up" },
        ws.name,
        runner.name
    );
    if let Err(e) = confirm(ctx, &summary).await {
        return format!("not scheduled: {e}");
    }

    let input = crate::storage::models::CronJobInput {
        id: existing.as_ref().map(|j| j.id.clone()),
        name: name.clone(),
        schedule: schedule.clone(),
        kind: "prompt".into(),
        payload: review_prompt(&ws.name, focus),
        targets_json: Some(
            serde_json::to_string(&runner.targets).unwrap_or_else(|_| "[]".into()),
        ),
        enabled,
        workspace_id: Some(ws.id.clone()),
        persona_id: Some(runner.id.clone()),
    };
    match ctx.db.upsert_cron_job(&input) {
        Ok(_) if !enabled => format!("Stopped the review of {}.", ws.name),
        Ok(_) => format!(
            "{name} is set: {schedule}, run by {}. Nothing else needs to happen — it will \
             report to you on its own.",
            runner.name
        ),
        Err(e) => format!("error scheduling the review: {e}"),
    }
}

/// Everything needed to decide what a project's team should do next.
///
/// The pieces existed separately — numbers, per-agent work, what changed on the servers
/// — and separately they do not answer the question. "The team shipped nine fixes" and
/// "revenue fell 12%" are each unremarkable; together they say the work is not touching
/// whatever is actually wrong, which is the finding worth acting on.
///
/// So it ends by naming the decision rather than leaving a pile of data. An agent handed
/// a report and no question tends to summarise it back.
async fn project_review(ctx: &ToolContext, args: &Value) -> String {
    let all_ws = ctx.db.list_workspaces().unwrap_or_default();
    let named = args.get("project").and_then(|v| v.as_str()).map(str::trim);
    let ws = match named.filter(|n| !n.is_empty()) {
        Some(n) => match all_ws.iter().find(|w| w.name.eq_ignore_ascii_case(n) || w.id == n) {
            Some(w) => w.clone(),
            None => {
                return format!(
                    "error: no project called {n:?}. Known: {}",
                    all_ws.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            }
        },
        None => match ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) {
            Some(id) => match all_ws.iter().find(|w| w.id == id) {
                Some(w) => w.clone(),
                None => return "error: the open project no longer exists".into(),
            },
            None => return "error: no project is open, so name one with `project`".into(),
        },
    };
    let days = args.get("days").and_then(|v| v.as_i64()).unwrap_or(7).clamp(1, 90);

    let mut out = format!("# {} — last {days} day(s)\n", ws.name);

    // 1. Whether it is earning more or less. First, because it is what the rest is for.
    out.push_str("\n## Numbers\n");
    out.push_str(&crate::ai::metrics_tools::trend_for(
        ctx,
        &ws.id,
        &ws.name,
        None,
        days,
    ));

    // 2. Who did what, and what came of it.
    let personas = ctx.db.list_personas().unwrap_or_default();
    let team: Vec<&crate::storage::models::Persona> = personas
        .iter()
        .filter(|p| p.workspace_id.as_deref() == Some(ws.id.as_str()) && p.enabled)
        .collect();
    let since = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();

    out.push_str("\n## The team\n");
    if team.is_empty() {
        out.push_str(
            "Nobody is assigned to this project. Whatever happened here was done by a \
             company-wide agent or by the user — worth fixing if this project is meant to \
             look after itself.\n",
        );
    }
    for p in &team {
        let tasks = ctx.db.agent_tasks_since(&p.id, &since).unwrap_or_default();
        let done = tasks.iter().filter(|t| t.status == "done").count();
        let stuck = tasks.iter().filter(|t| t.status == "blocked").count();
        out.push_str(&format!(
            "\n- **{}**: {} task(s) — {done} finished, {stuck} blocked\n",
            p.name,
            tasks.len()
        ));
        for t in tasks.iter().take(5) {
            out.push_str(&format!("  · [{}] {}\n", t.status, t.title));
            if let Some(o) = t.outcome.as_deref().filter(|o| !o.trim().is_empty()) {
                out.push_str(&format!("      → {}\n", o.trim().lines().next().unwrap_or("")));
            }
        }
        if tasks.is_empty() {
            // An idle agent is a finding: either it has nothing to do, or nobody is
            // giving it anything.
            out.push_str("  · nothing this period\n");
        }
    }

    // 3. The repository, before anything else about the work: a stale pull request and
    // an uncommitted tree are the two ways a period's work quietly stops counting.
    out.push_str("\n## Repository\n");
    match crate::ai::repo::status_of(&ctx.db, &ctx.sessions, &ws.id).await {
        Ok(st) => {
            out.push_str(&format!("{}\n", st.summary()));
            if st.work_at_risk() {
                out.push_str(
                    "Work exists in one place only — commit and push it (repo_save) before \
                     anything else.\n",
                );
            }
        }
        Err(e) => out.push_str(&format!("(could not read: {e})\n")),
    }
    let prs = crate::ai::repo::pull_requests(&ctx.db, &ctx.sessions, &ws.id).await;
    let stale: Vec<_> = prs.iter().filter(|p| p.is_stale()).collect();
    out.push_str(&format!("Open pull requests: {}", prs.len()));
    if stale.is_empty() {
        out.push_str("\n");
    } else {
        out.push_str(&format!(", {} stale:\n", stale.len()));
        for p in stale.iter().take(8) {
            out.push_str(&format!("- {}\n", p.line()));
        }
    }

    // 4. What actually changed, and what is still open.
    match crate::commands::project::history(&ctx.db, &ctx.sessions, &ws.id, 100).await {
        Ok(h) => {
            out.push_str(&format!(
                "\n## Changes\n{} file change(s); {} commit(s){}\n",
                h.changes.len(),
                h.commits.len(),
                h.branch.map(|b| format!(" on {b}")).unwrap_or_default()
            ));
            for c in h.commits.iter().take(8) {
                out.push_str(&format!("- {} {}\n", c.sha, c.subject));
            }
            if let Some(note) = h.git_note {
                out.push_str(&format!("- {note}\n"));
            }
            let open: Vec<&crate::storage::models::GoalSession> = h
                .tasks
                .iter()
                .filter(|t| matches!(t.status.as_str(), "active" | "waiting" | "blocked"))
                .collect();
            out.push_str(&format!("\n## Still open ({})\n", open.len()));
            for t in open.iter().take(10) {
                out.push_str(&format!("- [{}] {}\n", t.status, t.title));
            }
            if open.is_empty() {
                out.push_str("- (nothing in flight)\n");
            }
        }
        Err(e) => out.push_str(&format!("\n## Changes\n(could not read: {e})\n")),
    }

    out.push_str(
        "\n## What to decide\n\
         Read the numbers against the work, not on their own. If a metric fell while the \
         team shipped plenty, the work is not touching what is actually wrong — say what \
         you think the cause is and what would test that, rather than proposing more of \
         the same. If a metric rose, say which change you think did it, so it can be \
         repeated. If there are no numbers at all, that is the first thing to fix: record \
         them with metric_record, because without them none of this is answerable.\n\
         Then act: agent_delegate the work you decided on, agent_hire someone if the team \
         is missing a skill, agent_dismiss or agent_hire (same name) to change a remit \
         that is not paying off. Report what you decided upward with agent_report.\n",
    );
    out
}

/// Whether a finished task's report matches what the machine recorded.
///
/// An agent reports its own work, so every "done" is a claim. Most are true, but "most"
/// is not something to build on when the work happened unattended — and the failure is
/// quiet: a task marked done, a summary that reads well, and nothing behind it.
///
/// None of these facts come from the agent making the claim. The commands are in the
/// CLI's own transcript, the files are in the edit journal, the commits are in git. Put
/// side by side, the combinations that cannot both be true become obvious, and this
/// names them rather than leaving a reader to spot them.
async fn task_audit(ctx: &ToolContext, args: &Value) -> String {
    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("").trim();
    let Some(goal) = ctx.db.get_goal(task_id).ok().flatten() else {
        return format!("error: no task {task_id}");
    };
    let who = goal
        .persona_id
        .as_deref()
        .map(|id| display_name(ctx, Some(id)))
        .unwrap_or_else(|| "the main agent".into());

    // What was claimed.
    let claim = goal
        .outcome
        .as_deref()
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .unwrap_or("(nothing was recorded about the result)");

    // What was recorded, by things the agent does not write.
    let changes = ctx
        .db
        .list_file_changes(Some(&format!("goal:{task_id}")), None, 500)
        .unwrap_or_default();
    let commands_run = match crate::ai::providers::cli::get_cli_conversation(&format!("goal:{task_id}")) {
        Some(session) => {
            let cmd = crate::ai::transcript::read_command(&session);
            let out = match ctx.targets.first() {
                Some(v) => ctx.sessions.run_command(v, &cmd).await.map(|o| o.stdout).unwrap_or_default(),
                None => crate::proc::quiet_command("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_default(),
            };
            if out.trim().starts_with("NOT_FOUND") || out.trim().is_empty() {
                None
            } else {
                let body = out.split_once('\n').map(|(_, r)| r).unwrap_or(&out);
                Some(crate::ai::transcript::parse(body))
            }
        }
        None => None,
    };

    let mut out = format!(
        "Audit of \"{}\" ({}), run by {who}, status {}\n\nWhat it reported:\n{claim}\n",
        goal.title, task_id, goal.status
    );

    out.push_str(&format!("\nWhat was recorded:\n- {} file change(s)\n", changes.len()));
    for c in changes.iter().take(10) {
        out.push_str(&format!("  · {} {}\n", if c.is_new { "created" } else { "edited" }, c.path));
    }
    let cmd_count = match &commands_run {
        Some(entries) => {
            let cmds = crate::ai::transcript::commands(entries);
            out.push_str(&format!("- {} command(s) run\n", cmds.len()));
            for c in cmds.iter().rev().take(10).rev() {
                out.push_str(&format!("  · {c}\n"));
            }
            Some(cmds.len())
        }
        None => {
            // Absence of a transcript is not evidence of absence of work, and reporting
            // it as "0 commands" would accuse an agent of doing nothing on the strength
            // of a cleaned-up log file.
            out.push_str("- commands: no transcript available (the CLI cleans them up, and \
                          not every provider keeps one) — this is unknown, not zero\n");
            None
        }
    };

    // The combinations that cannot both be true.
    let mut flags: Vec<String> = Vec::new();
    if goal.status == "done" {
        flags.extend(done_contradictions(&WorkRecord {
            evidence: claim.to_string(),
            file_changes: changes.len(),
            kanban_notes: kanban_note_count(&goal.kanban_json),
            commands: cmd_count,
        }));
        if goal.cycles <= 1 && changes.is_empty() && cmd_count.unwrap_or(0) <= 1 {
            flags.push(
                "finished in one cycle with almost nothing recorded — worth reading the \
                 transcript with session_read before relying on it."
                    .into(),
            );
        }
    }

    if flags.is_empty() {
        out.push_str(
            "\nNothing contradicts the report. That is not proof it is right — read the \
             commands above against what it claims, because only you can tell whether they \
             do what it says they did.\n",
        );
    } else {
        out.push_str("\nDoes not add up:\n");
        for f in &flags {
            out.push_str(&format!("- {f}\n"));
        }
        out.push_str(
            "\nOpen it with session_read before accepting this. If the report is wrong, say \
             so to whoever wrote it and to the user — a false report that goes uncorrected is \
             built on.\n",
        );
    }
    out
}

/// One agent's record over a window: what it was asked, what came of it, what it
/// changed, and what it said.
///
/// Assembled rather than logged separately, because a second log would be a second
/// thing to keep true. The pieces already exist — tasks carry a persona and an outcome,
/// file changes carry the session id its runs use, messages carry both ends — and what
/// was missing was somewhere to read them together, per agent, over a period.
fn agent_activity(ctx: &ToolContext, args: &Value) -> String {
    let who = args.get("agent").and_then(|v| v.as_str()).unwrap_or("").trim();
    let Some(p) = crate::ai::persona::resolve(&ctx.db, who) else {
        return format!(
            "error: no agent named {who:?}.\n{}",
            crate::ai::persona::format_catalog(&team(ctx))
        );
    };
    let days = args.get("days").and_then(|v| v.as_i64()).unwrap_or(7).clamp(1, 90);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let since_rfc = since.to_rfc3339();
    // The edit journal stores epoch milliseconds, not RFC 3339.
    let since_ms = since.timestamp_millis();

    let tasks = ctx.db.agent_tasks_since(&p.id, &since_rfc).unwrap_or_default();
    let changes = ctx
        .db
        .agent_file_changes_since(&p.id, since_ms, 200)
        .unwrap_or_default();
    let messages = ctx.db.agent_messages_since(&p.id, &since_rfc, 100).unwrap_or_default();

    let mut out = format!("{} — last {days} day(s)", p.name);
    if let Some(ws) = p
        .workspace_id
        .as_deref()
        .and_then(|id| ctx.db.get_workspace(id).ok().flatten())
    {
        out.push_str(&format!(" on {}", ws.name));
    }
    out.push_str("\n");

    if tasks.is_empty() && changes.is_empty() && messages.is_empty() {
        // Said plainly: an agent that did nothing is a finding, not an empty report.
        out.push_str("\nNothing in this period — no tasks, no changes, nothing said.\n");
        return out;
    }

    out.push_str(&format!("\nTasks ({}):\n", tasks.len()));
    for t in tasks.iter().take(25) {
        out.push_str(&format!("- [{}] {}", t.status, t.title));
        if let Some(o) = t.outcome.as_deref().filter(|o| !o.trim().is_empty()) {
            out.push_str(&format!("\n    result: {}", o.trim().lines().next().unwrap_or("")));
        }
        out.push('\n');
    }
    if tasks.is_empty() {
        out.push_str("- (none)\n");
    }

    out.push_str(&format!("\nFiles changed ({}):\n", changes.len()));
    for c in changes.iter().take(25) {
        out.push_str(&format!(
            "- {} {} ({})\n",
            if c.is_new { "created" } else { "edited" },
            c.path,
            c.label
        ));
    }
    if changes.is_empty() {
        out.push_str("- (none)\n");
    }

    out.push_str(&format!("\nSaid ({}):\n", messages.len()));
    for m in messages.iter().take(20) {
        let dir = if m.from_id.as_deref() == Some(p.id.as_str()) { "→" } else { "←" };
        let other = if m.from_id.as_deref() == Some(p.id.as_str()) {
            display_name(ctx, m.to_id.as_deref())
        } else {
            display_name(ctx, m.from_id.as_deref())
        };
        out.push_str(&format!(
            "- {dir} {other} [{}]: {}\n",
            m.kind,
            m.body.lines().next().unwrap_or("").chars().take(140).collect::<String>()
        ));
    }
    if messages.is_empty() {
        out.push_str("- (none)\n");
    }
    out
}

/// Every project, its team, and what that team is doing.
///
/// The point of a single person to talk to is that they can answer for everything. With
/// one team per project, an agent that could only see the project in front of it would
/// have to be asked about each one in turn — which is the thing having a chief of staff
/// is supposed to remove.
fn teams_overview(ctx: &ToolContext) -> String {
    let workspaces = ctx.db.list_workspaces().unwrap_or_default();
    let all = ctx.db.list_personas().unwrap_or_default();
    if workspaces.is_empty() {
        return "There are no projects yet. Create one on the canvas, then agents can be \
                assigned to it."
            .into();
    }

    let mut out = String::from("Projects and their teams:\n");
    for ws in &workspaces {
        let team: Vec<&crate::storage::models::Persona> = all
            .iter()
            .filter(|p| p.workspace_id.as_deref() == Some(ws.id.as_str()) && p.enabled)
            .collect();
        let tasks = ctx.db.list_goals_for_workspace(Some(&ws.id)).unwrap_or_default();
        let running = tasks
            .iter()
            .filter(|t| matches!(t.status.as_str(), "active" | "waiting" | "intake"))
            .count();

        out.push_str(&format!("\n## {}\n", ws.name));
        if team.is_empty() {
            // Worth saying rather than showing an empty line: a project with no team is
            // one nobody is looking after, which is exactly what this is for surfacing.
            out.push_str("  team: nobody assigned — work here has to be done by a company-wide agent\n");
        } else {
            for p in &team {
                let manager = p
                    .reports_to
                    .as_deref()
                    .map(|id| display_name(ctx, Some(id)))
                    .unwrap_or_else(|| "the user".into());
                out.push_str(&format!(
                    "  - {}{} → reports to {manager}\n",
                    p.name,
                    if p.role.trim().is_empty() { String::new() } else { format!(" ({})", p.role.trim()) }
                ));
            }
        }
        out.push_str(&format!(
            "  tasks: {running} in flight, {} finished\n",
            tasks.len().saturating_sub(running)
        ));
        for t in tasks.iter().filter(|t| t.status == "active").take(3) {
            out.push_str(&format!("    · {}\n", t.title));
        }
    }

    let house: Vec<&str> = all
        .iter()
        .filter(|p| p.workspace_id.is_none() && p.enabled)
        .map(|p| p.name.as_str())
        .collect();
    out.push_str(&format!(
        "\nCompany-wide (answer on any project): {}\n\nTo hand work to another \
         project's team, call agent_delegate with `project` set to that project's name.\n",
        if house.is_empty() { "none".into() } else { house.join(", ") }
    ));
    out
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
    fn team_names_carry_the_project_so_several_leads_can_exist() {
        // Names are unique across every project, so "Lead" on its own can only ever
        // belong to one of them — and `agent_send "Lead"` would be a coin toss.
        assert_eq!(team_member_name("CSB", "lead"), "CSB Lead");
        assert_eq!(team_member_name("GameQuery", "engineer"), "GameQuery Engineer");
        assert_ne!(
            team_member_name("CSB", "lead"),
            team_member_name("GameQuery", "lead")
        );
    }

    #[test]
    fn a_long_or_multiword_project_still_gives_a_usable_name() {
        // The name is typed by a person addressing the agent, so it has to stay short
        // enough to say.
        let n = team_member_name("counter-strike-boost.com production", "ops");
        assert!(n.ends_with(" Ops"), "{n}");
        assert!(n.len() < 30, "{n}");
    }

    #[test]
    fn a_reviewer_cannot_be_given_the_run_of_the_place_by_default() {
        // A reviewer that can change things is not a reviewer, and an engineer that can
        // do anything unattended is a much bigger decision than "add an engineer"
        // sounds like. Defaults start narrow; the user widens them deliberately.
        assert_eq!(role_defaults("reviewer").trust, Some("approve"));
        assert_eq!(role_defaults("qa").trust, Some("approve"));
        assert_eq!(role_defaults("engineer").trust, Some("allowlist"));
        // An unrecognised role is the most cautious of all — nobody said what it does.
        assert_eq!(role_defaults("wizard").trust, Some("approve"));
        // The lead inherits the global setting rather than being pinned narrow: it is
        // the one the user actually converses with.
        assert_eq!(role_defaults("lead").trust, None);
    }

    #[test]
    fn each_role_is_confined_to_the_part_of_the_tree_it_owns() {
        // The roster is only a roster if the roles differ in what they can do. Two
        // engineers that may both rewrite the whole repository are one engineer twice.
        assert_eq!(role_defaults("backend").paths, ["src-tauri/**"]);
        assert_eq!(role_defaults("frontend").paths, ["src/**"]);
        assert!(role_defaults("qa").paths.iter().any(|p| p.contains("test")));
        assert!(role_defaults("architect").paths.iter().any(|p| p.contains("md")));
        // The lead and the researcher are held by their tools, not their paths: neither
        // list contains anything that writes.
        for role in ["lead", "researcher"] {
            let d = role_defaults(role);
            assert!(!d.tools.is_empty(), "{role} needs a tool list");
            assert!(
                !d.tools.iter().any(|t| t.contains("write") || t.contains("edit")),
                "{role} was given a write tool"
            );
        }
        // And the default team is a team: one of each, not three of one.
        assert_eq!(DEFAULT_ROLES.len(), 5);
        assert!(DEFAULT_ROLES.contains(&"frontend"));
    }

    #[test]
    fn every_declared_tool_is_recognised_and_dispatchable() {
        // A tool the model can see but the router does not know produces "unknown tool"
        // at runtime, which stays invisible until someone happens to ask for it.
        for def in definitions() {
            assert!(is_persona_tool(&def.name), "{} is not routed", def.name);
        }
    }

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

    fn persona(name: &str, id: &str) -> Persona {
        Persona {
            id: id.into(),
            name: name.into(),
            role: "does the job".into(),
            instructions: String::new(),
            targets: vec![],
            safety_mode: None,
            provider_id: None,
            model: None,
            enabled: true,
            reports_to: None,
            workspace_id: None,
            created_at: None,
            updated_at: None,
            allowed_paths: Vec::new(),
            allowed_tools: Vec::new(),
        }
    }

    /// Two projects, each with an engineer, plus one company-wide agent.
    fn two_projects() -> (Vec<Persona>, Vec<Persona>) {
        let mut a = persona("A Engineer", "pa");
        a.workspace_id = Some("ws-a".into());
        let mut b = persona("B Engineer", "pb");
        b.workspace_id = Some("ws-b".into());
        let orchestrator = persona("Orchestrator", "po");
        let all = vec![a.clone(), b.clone(), orchestrator.clone()];
        // What an agent working on project A can address: its own team plus the
        // company-wide agents, exactly as `team()` builds it at the call sites.
        let known = crate::ai::persona::team_for(&all, Some("ws-a"))
            .into_iter()
            .cloned()
            .collect();
        (all, known)
    }

    #[test]
    fn delegating_to_a_named_agent_on_another_project_is_refused() {
        // The hole this closes: the team list was built and then ignored, because
        // naming an agent went to `resolve`, which searches the whole database. One
        // project's lead could hand work to another project's engineer by typing their
        // name, and nothing anywhere said a word about it.
        let (all, known) = two_projects();
        let err = find_addressable(&known, &all, "B Engineer", true).unwrap_err();
        assert!(err.contains("another project"), "{err}");
        // Their own team and the company-wide agents still resolve.
        assert_eq!(find_addressable(&known, &all, "A Engineer", true).unwrap().id, "pa");
        assert_eq!(find_addressable(&known, &all, "Orchestrator", true).unwrap().id, "po");
        // A name nobody has is a different refusal, and says so.
        assert!(find_addressable(&known, &all, "Nobody", true)
            .unwrap_err()
            .contains("no agent named"));
    }

    #[test]
    fn agent_send_cannot_reach_another_project_either() {
        // Same helper, same refusal: a message is work, so reaching across by name was
        // the identical hole in a second tool. The unscoped turn — the agent the user
        // talks to over chat, with no project open — is the one exception, and it is
        // what makes "tell the CSB engineer to stop" work from a phone.
        let (all, known) = two_projects();
        assert!(find_addressable(&known, &all, "B Engineer", true).is_err());
        assert_eq!(
            find_addressable(&known, &all, "B Engineer", false).unwrap().id,
            "pb"
        );
    }

    #[test]
    fn stopping_a_task_on_another_project_is_refused() {
        // Task control is authority, and authority is scoped the same way work is.
        assert!(!task_visible(Some("ws-b"), Some("ws-a")));
        assert!(task_visible(Some("ws-a"), Some("ws-a")));
        // A turn with no project sees every task — that is the orchestrator, and being
        // able to stop anything from a phone is the whole point of these tools.
        assert!(task_visible(Some("ws-b"), None));
        assert!(task_visible(None, None));
        // A task filed under nothing is not visible from inside a project: it belongs
        // to whoever started it, not to whichever project happens to be open.
        assert!(!task_visible(None, Some("ws-a")));
        assert!(!task_visible(Some(""), Some("ws-a")));
    }

    fn goal(pid: &str, status: &str, when: &str) -> GoalSession {
        GoalSession {
            id: format!("g-{pid}-{status}"),
            title: "t".into(),
            raw_request: "r".into(),
            spec_json: "{}".into(),
            status: status.into(),
            kanban_json: "[]".into(),
            memory_json: "{}".into(),
            next_check_at: None,
            cycles: 1,
            created_at: Some(when.into()),
            updated_at: Some(when.into()),
            finished_at: if status == "done" {
                Some(when.into())
            } else {
                None
            },
            persona_id: Some(pid.into()),
            workspace_id: None,
            outcome: None,
            request_id: None,
            reported_at: None,
            pr_number: None,
            approval_state: None,
        }
    }

    #[test]
    fn idle_duty_skips_anyone_already_on_a_task() {
        let people = vec![persona("Ada", "ada"), persona("Bruno", "bruno")];
        let goals = vec![goal("ada", "active", "2026-01-01 00:00:00")];
        let picks = idle_duty_picks(
            &people,
            &goals,
            &HashSet::new(),
            parse_goal_ts("2026-01-01 12:00:00").unwrap(),
            Duration::minutes(20),
            3,
        );
        assert_eq!(picks, vec!["bruno".to_string()]);
    }

    #[test]
    fn idle_duty_respects_cooldown_unless_mail_is_waiting() {
        let people = vec![persona("Ada", "ada"), persona("Bruno", "bruno")];
        let now = parse_goal_ts("2026-01-01 12:00:00").unwrap();
        let goals = vec![
            goal("ada", "done", "2026-01-01 11:50:00"),
            goal("bruno", "done", "2026-01-01 11:50:00"),
        ];
        let picks = idle_duty_picks(
            &people,
            &goals,
            &HashSet::new(),
            now,
            Duration::minutes(20),
            3,
        );
        assert!(picks.is_empty(), "{picks:?}");
        let mut unread = HashSet::new();
        unread.insert("ada".into());
        let picks = idle_duty_picks(&people, &goals, &unread, now, Duration::minutes(20), 3);
        assert_eq!(picks, vec!["ada".to_string()]);
    }

    #[test]
    fn idle_duty_picks_the_one_who_has_been_idle_longest() {
        let people = vec![
            persona("Ada", "ada"),
            persona("Bruno", "bruno"),
            persona("Cypher", "cypher"),
        ];
        let goals = vec![
            goal("ada", "done", "2026-01-01 08:00:00"),
            goal("bruno", "done", "2026-01-01 06:00:00"),
            goal("cypher", "done", "2026-01-01 07:00:00"),
        ];
        let picks = idle_duty_picks(
            &people,
            &goals,
            &HashSet::new(),
            parse_goal_ts("2026-01-01 12:00:00").unwrap(),
            Duration::minutes(20),
            2,
        );
        assert_eq!(picks, vec!["bruno".to_string(), "cypher".to_string()]);
    }

    #[test]
    fn a_done_with_nothing_behind_it_is_refused() {
        let flags = done_contradictions(&WorkRecord {
            evidence: "shipped it, tests pass".into(),
            file_changes: 0,
            kanban_notes: 0,
            commands: Some(0),
        });
        assert!(!flags.is_empty(), "{flags:?}");
        assert!(flags.iter().any(|f| f.contains("nothing was changed")), "{flags:?}");
    }

    #[test]
    fn a_done_with_a_file_change_and_evidence_is_accepted() {
        let flags = done_contradictions(&WorkRecord {
            evidence: "wrote deploy.sh and ran it; health 200".into(),
            file_changes: 1,
            kanban_notes: 0,
            commands: None,
        });
        assert!(flags.is_empty(), "{flags:?}");
    }

    #[test]
    fn a_qa_verdict_with_board_notes_is_accepted_without_files() {
        // Reviewers are read-only. The board note is the record; demanding a file
        // change would force them to mutate something just to be believed.
        let flags = done_contradictions(&WorkRecord {
            evidence: "APPROVE: migration additive, 23/23 tests passed in pgt-app-1".into(),
            file_changes: 0,
            kanban_notes: 1,
            commands: None,
        });
        assert!(flags.is_empty(), "{flags:?}");
    }

    #[test]
    fn empty_evidence_is_not_a_result() {
        let flags = done_contradictions(&WorkRecord {
            evidence: "  ".into(),
            file_changes: 3,
            kanban_notes: 1,
            commands: Some(4),
        });
        assert!(flags.iter().any(|f| f.contains("no evidence")), "{flags:?}");
    }

    #[test]
    fn kanban_note_count_ignores_empty_cards() {
        let json = r#"[{"id":"1","column":"in_progress","title":"x","result":""},{"id":"2","column":"done","title":"y","result":"df / is 71%"}]"#;
        assert_eq!(kanban_note_count(json), 1);
        assert_eq!(kanban_note_count("[]"), 0);
    }

    #[test]
    fn duty_task_names_the_person_and_forbids_sitting_idle() {
        let p = persona("Bruno", "bruno");
        let t = duty_task(&p);
        assert!(t.contains(DUTY_MARK), "{t}");
        assert!(t.contains("Bruno"), "{t}");
        assert!(t.starts_with("Standing work on your remit"), "{t}");
        assert!(t.contains("sit idle"), "{t}");
    }
}
