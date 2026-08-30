//! Reading back what an agent actually did, rather than what it said it did.
//!
//! An agent reports its own work, and a report is a claim. Most of the time it is a
//! true one, but "most of the time" is not a property you can build on when the work is
//! unattended and the user is asleep — and the failure is quiet: a task marked done, a
//! summary that reads well, and nothing behind it.
//!
//! The CLI keeps a full transcript of every session: every prompt, every reply, every
//! command it ran and what came back. That is the record, and it is not written by the
//! agent making the claim. So an agent — or a reviewer, or the user — can open somebody
//! else's session and read what happened, then compare.
//!
//! Condensed rather than returned whole, because a session is megabytes and the
//! question is almost always "what did it actually run".

use serde::Serialize;

/// One thing that happened in a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Entry {
    /// What the agent was asked.
    Asked(String),
    /// What it said back.
    Said(String),
    /// A tool it used, and the part of the input worth reading.
    Did { tool: String, detail: String },
}

impl Entry {
    pub fn line(&self) -> String {
        match self {
            Entry::Asked(t) => format!("USER: {t}"),
            Entry::Said(t) => format!("AGENT: {t}"),
            Entry::Did { tool, detail } if detail.is_empty() => format!("  [{tool}]"),
            Entry::Did { tool, detail } => format!("  [{tool}] {detail}"),
        }
    }
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    let cut = crate::ai::text::floor_char_boundary(s, max.min(s.len()));
    if cut < s.len() {
        format!("{}…", &s[..cut])
    } else {
        s.to_string()
    }
}

/// The part of a tool call worth reading in a summary.
///
/// A command is the whole point of the record: "it ran Bash" says nothing, and
/// `rm -rf /srv/app` says everything.
fn tool_detail(input: &serde_json::Value) -> String {
    for key in ["command", "file_path", "path", "pattern", "url", "query", "description"] {
        if let Some(v) = input.get(key).and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                return clip(v, 300);
            }
        }
    }
    String::new()
}

/// Condense a session's JSONL into what happened, oldest first.
///
/// Tolerant of records it does not recognise: the format grows, and a reader that
/// stopped at the first unfamiliar line would report an empty session — which is the
/// same thing it reports for a session where nothing happened, and those must never
/// look alike.
pub fn parse(jsonl: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in jsonl.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        // Sub-agent chatter belongs to its own run, not this one.
        if v.get("isSidechain").and_then(|s| s.as_bool()) == Some(true) {
            continue;
        }
        let Some(message) = v.get("message") else { continue };
        match message.get("role").and_then(|r| r.as_str()) {
            Some("user") => {
                // A user turn is a plain string, or blocks when it carries tool results.
                if let Some(t) = message.get("content").and_then(|c| c.as_str()) {
                    let t = t.trim();
                    // Continuation preambles are machinery, not something anyone asked.
                    if !t.is_empty() && !t.starts_with("This session is being continued") {
                        out.push(Entry::Asked(clip(t, 400)));
                    }
                }
            }
            Some("assistant") => {
                for block in message
                    .get("content")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
                {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                if !t.trim().is_empty() {
                                    out.push(Entry::Said(clip(t, 400)));
                                }
                            }
                        }
                        Some("tool_use") => out.push(Entry::Did {
                            tool: block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("tool")
                                .to_string(),
                            detail: block.get("input").map(tool_detail).unwrap_or_default(),
                        }),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every command a session ran, in order. The shortest answer to "what did it do".
pub fn commands(entries: &[Entry]) -> Vec<&str> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Did { tool, detail } if tool.eq_ignore_ascii_case("bash") && !detail.is_empty() => {
                Some(detail.as_str())
            }
            _ => None,
        })
        .collect()
}

/// Shell that finds a session's transcript wherever the CLI put it and prints it.
///
/// Located by searching rather than by rebuilding the path: the CLI derives the
/// directory from the working directory it was started in, slugified, and reproducing
/// that rule here would break the day it changes.
pub fn read_command(session_id: &str) -> String {
    let id = crate::ssh::remote_ops::shell_quote(session_id);
    format!(
        "id={id}; \
         f=$(find \"$HOME/.claude/projects\" -name \"$id.jsonl\" -type f 2>/dev/null | head -1); \
         if [ -z \"$f\" ]; then echo 'NOT_FOUND'; exit 0; fi; \
         echo \"FOUND $f\"; \
         tail -c 2000000 \"$f\""
    )
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

pub fn definitions() -> Vec<crate::ai::provider::ToolDef> {
    use serde_json::json;
    vec![
        crate::ai::provider::ToolDef {
            name: "session_read".into(),
            description: "Open the transcript of a delegated task and read what actually happened in it: what the agent was asked, what it said, and every command it ran. Yours or another agent's. Use it to check a report against the work — a claim that something was fixed is worth exactly as much as the commands behind it — and to pick up a task somebody else left half-done without asking them to explain it again."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "The delegated task, from agent_check or agent_delegate."},
                    "commands_only": {"type": "boolean", "description": "Just the commands it ran, which is usually the question. Default false."},
                    "limit": {"type": "integer", "description": "Max entries to return, newest kept (default 120)."}
                },
                "required": ["task_id"]
            }),
        },
    ]
}

pub fn is_transcript_tool(name: &str) -> bool {
    name == "session_read"
}

/// Reading a record changes nothing.
pub fn tool_is_mutating(_name: &str) -> bool {
    false
}

pub async fn dispatch(
    ctx: &crate::ai::tools::ToolContext,
    _name: &str,
    args: &serde_json::Value,
) -> String {
    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("").trim();
    if task_id.is_empty() {
        return "error: session_read needs a 'task_id'".into();
    }
    let Some(goal) = ctx.db.get_goal(task_id).ok().flatten() else {
        return format!("error: no task {task_id}");
    };
    // The CLI keeps its transcript under the session id xConsole ran the task with.
    let Some(cli_session) = crate::ai::providers::cli::get_cli_conversation(&format!("goal:{task_id}"))
    else {
        return format!(
            "No transcript for \"{}\": it ran on a provider that keeps none, or the run \
             predates this being recorded. What it changed is still in project_history, and \
             what it claimed is in agent_check.",
            goal.title
        );
    };

    // Where it ran. A remote CLI keeps the transcript on that server, not here.
    let vps = goal
        .spec_json
        .parse::<serde_json::Value>()
        .ok()
        .and_then(|v| {
            v.get("vps_targets")
                .and_then(|t| t.as_array())
                .and_then(|a| a.first())
                .and_then(|t| t.as_str())
                .map(str::to_string)
        })
        .or_else(|| ctx.targets.first().cloned());

    let cmd = read_command(&cli_session);
    let out = match &vps {
        Some(v) => match ctx.sessions.run_command(v, &cmd).await {
            Ok(o) => o.stdout,
            Err(e) => return format!("error reading the transcript on {v}: {e}"),
        },
        None => match crate::proc::quiet_command("sh").arg("-c").arg(&cmd).output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(e) => return format!("error reading the transcript: {e}"),
        },
    };
    if out.trim().starts_with("NOT_FOUND") || out.trim().is_empty() {
        return format!(
            "The transcript for \"{}\" (session {cli_session}) is not on that machine any \
             more. Sessions are kept by the CLI, not by xConsole, and they are cleaned up.",
            goal.title
        );
    }

    let body = out.split_once('\n').map(|(_, rest)| rest).unwrap_or(&out);
    let entries = parse(body);
    if entries.is_empty() {
        return format!("The transcript for \"{}\" is empty.", goal.title);
    }

    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(120).clamp(1, 500) as usize;
    if args.get("commands_only").and_then(|v| v.as_bool()).unwrap_or(false) {
        let cmds = commands(&entries);
        return format!(
            "\"{}\" ran {} command(s):\n{}",
            goal.title,
            cmds.len(),
            cmds.iter()
                .rev()
                .take(limit)
                .rev()
                .map(|c| format!("- {c}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // Newest kept: a long session's beginning is setup, and the end is what it claims.
    let shown: Vec<String> = entries.iter().rev().take(limit).rev().map(|e| e.line()).collect();
    format!(
        "\"{}\" — {} entries, {} command(s){}:\n{}",
        goal.title,
        entries.len(),
        commands(&entries).len(),
        if entries.len() > limit { format!(", showing the last {limit}") } else { String::new() },
        shown.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = r#"{"type":"custom-title","customTitle":"x"}
{"type":"user","message":{"role":"user","content":"delete the fivem containers"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I'll stop them first."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"docker compose down -v"}}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"rm -rf /root/fivem"}}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done - containers and data removed."}]}}"#;

    #[test]
    fn a_session_reads_back_as_what_was_asked_said_and_run() {
        let e = parse(SESSION);
        assert_eq!(e[0], Entry::Asked("delete the fivem containers".into()));
        assert_eq!(e[1], Entry::Said("I'll stop them first.".into()));
        assert!(matches!(&e[2], Entry::Did { tool, .. } if tool == "Bash"));
        assert_eq!(e.last(), Some(&Entry::Said("Done - containers and data removed.".into())));
    }

    #[test]
    fn the_commands_are_recoverable_on_their_own() {
        // This is the whole point: a claim of "I removed it" is checked against what
        // actually ran, and "it ran Bash" would answer nothing.
        let entries = parse(SESSION);
        let cmds = commands(&entries);
        assert_eq!(cmds, vec!["docker compose down -v", "rm -rf /root/fivem"]);
    }

    #[test]
    fn an_unfamiliar_record_does_not_empty_the_session() {
        // The format grows. A reader that stopped at the first line it did not know
        // would report an empty session — indistinguishable from one where nothing
        // happened, and those must never look alike.
        let mixed = format!("{{\"type\":\"brand-new-thing\",\"x\":1}}\nnot json at all\n{SESSION}");
        assert_eq!(parse(&mixed).len(), parse(SESSION).len());
    }

    #[test]
    fn a_subagents_own_run_is_not_counted_as_this_ones_work() {
        // Sidechain records belong to a sub-agent's session. Folding them in would
        // credit this agent with work it delegated, which is the opposite of checking
        // who did what.
        let with_side = format!(
            "{SESSION}\n{}",
            r#"{"isSidechain":true,"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"rm -rf /somewhere/else"}}]}}"#
        );
        let entries = parse(&with_side);
        let cmds = commands(&entries);
        assert!(!cmds.iter().any(|c| c.contains("/somewhere/else")), "{cmds:?}");
    }

    #[test]
    fn a_continuation_preamble_is_not_reported_as_something_the_user_asked() {
        // It is machinery the CLI inserts, and reading it back as a request makes the
        // record look like the user asked for a summary of themselves.
        let e = parse(
            r#"{"type":"user","message":{"role":"user","content":"This session is being continued from a previous conversation. Summary: ..."}}"#,
        );
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn a_long_command_is_clipped_but_still_identifiable() {
        let long = "x".repeat(1000);
        let e = parse(&format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","name":"Bash","input":{{"command":"rm -rf /srv/{long}"}}}}]}}}}"#
        ));
        match &e[0] {
            Entry::Did { detail, .. } => {
                assert!(detail.len() < 400, "not clipped: {}", detail.len());
                assert!(detail.starts_with("rm -rf /srv/"), "lost the identity: {detail}");
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
    }

}
