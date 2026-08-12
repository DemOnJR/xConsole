//! The self-improvement loop (ETAPA 29). After a turn finishes, look at what went
//! wrong — failed tool calls, the same tool retried with identical args, hitting the
//! iteration cap — distill a short lesson, and save it to memory. Because MEMORY.md
//! is injected into every future turn's prompt, the next attempt does better. The
//! agent "sees where it went wrong, saves the lesson, improves its memory, and
//! adjusts its tool usage."
//!
//! The analysis + distillation are PURE functions over the transcript (no I/O, no
//! LLM call) so they're fast, deterministic, and unit-testable. Only `reflect_and_save`
//! touches the filesystem (the memory file).

use std::collections::{HashMap, HashSet};

use crate::ai::provider::ChatMessage;
use crate::ai::{memory, AgentHome};

/// Memory bullets begin with this tag so reflection-written lessons are recognizable
/// (and de-duplicated) without disturbing user/agent-authored memory.
const LESSON_TAG: &str = "[lesson]";

/// Capability-gap bullets — the agent told the user it couldn't do something. These
/// prime the NEXT turn to research-and-build-a-skill (`learn_skill`) instead of
/// declining again. A prime, not a safety net (the in-prompt forcing function is that).
const GAP_TAG: &str = "[gap]";

/// Phrases that signal the agent declined / professed ignorance rather than acting.
const IGNORANCE_PHRASES: &[&str] = &[
    "i don't know how",
    "i do not know how",
    "i'm not sure how",
    "i am not sure how",
    "don't know how to",
    "not sure how to",
    "i can't do that",
    "i cannot do that",
    "i'm not able to",
    "i am not able to",
    "i don't have a way to",
    "i'm unable to",
    "i am unable to",
    "no idea how",
];

/// A failed tool invocation observed in a finished turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolFailure {
    pub tool: String,
    /// The call's argument KEY NAMES, never their values — see [`arg_shape`].
    pub args_brief: String,
    pub error: String,
}

/// What the reflection pass concluded about a finished turn.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TurnOutcome {
    pub failures: Vec<ToolFailure>,
    /// Tools called ≥2× with identical arguments in the same turn (thrashing).
    pub repeated_tools: Vec<String>,
    /// The turn used up its whole tool-iteration budget without settling.
    pub hit_max_iters: bool,
    /// Short snippets of user requests the agent declined / professed ignorance on —
    /// candidates for `learn_skill` next time.
    pub gaps: Vec<String>,
}

impl TurnOutcome {
    pub fn had_trouble(&self) -> bool {
        !self.failures.is_empty()
            || !self.repeated_tools.is_empty()
            || self.hit_max_iters
            || !self.gaps.is_empty()
    }
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>().trim().to_string()
}

/// The tool name + brief args that produced the result at `result_idx`, by matching
/// the result's `tool_call_id` back to the assistant message that issued the call.
/// Describe a tool call's arguments by their KEY NAMES only — never their values.
///
/// A lesson built from a failed call is appended to `agent/MEMORY.md` on disk and then
/// injected into the system prompt of every later turn. Storing the values meant a failed
/// `local_run_command{"command":"mysql -u root -pHUNTER2"}` became a permanent, repeatedly
/// re-transmitted copy of that password. `MEMORY_GUIDANCE` tells the model not to record
/// secrets, but this write is code, not the model's choice.
///
/// Redaction was the obvious fix and is not enough: the credential shapes that show up
/// here are `-pSECRET`, `--token SECRET` and bare positional arguments, and a pattern
/// aggressive enough to catch those also mangles `find -print` and `docker run -p3306`.
/// The key names carry the useful signal — which call shape failed — with nothing to leak.
fn arg_shape(arguments: &serde_json::Value) -> String {
    match arguments.as_object() {
        Some(map) if !map.is_empty() => map.keys().cloned().collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

fn call_for_result(messages: &[ChatMessage], result_idx: usize) -> Option<(String, String)> {
    let id = messages[result_idx].tool_call_id.as_deref()?;
    for m in messages[..result_idx].iter().rev() {
        if m.role != "assistant" {
            continue;
        }
        for tc in &m.tool_calls {
            if tc.id == id {
                return Some((tc.name.clone(), arg_shape(&tc.arguments)));
            }
        }
    }
    None
}

/// True when a tool result message represents an error. Mirrors the app's own
/// success/failure convention (`tools::dispatch`: `ok = !result.starts_with("error:")`).
fn is_error_result(m: &ChatMessage) -> bool {
    m.role == "tool" && m.content.trim_start().to_lowercase().starts_with("error:")
}

/// Inspect a finished turn's transcript for mistakes worth learning from. Pure.
pub fn analyze_turn(messages: &[ChatMessage], iters_used: usize, max_iters: usize) -> TurnOutcome {
    let mut out = TurnOutcome::default();

    // Detect thrashing: the same (tool name + exact args) issued more than once.
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for m in messages {
        if m.role != "assistant" {
            continue;
        }
        for tc in &m.tool_calls {
            let key = format!("{}\u{0}{}", tc.name, tc.arguments);
            counts.entry(key).or_insert((tc.name.clone(), 0)).1 += 1;
        }
    }
    let mut repeated: HashSet<String> = HashSet::new();
    for (_, (name, n)) in counts {
        if n >= 2 {
            repeated.insert(name);
        }
    }
    out.repeated_tools = repeated.into_iter().collect();
    out.repeated_tools.sort();

    // Collect failed tool results, de-duplicated by (tool, error first line).
    let mut seen: HashSet<String> = HashSet::new();
    for (idx, m) in messages.iter().enumerate() {
        if !is_error_result(m) {
            continue;
        }
        let (tool, args_brief) =
            call_for_result(messages, idx).unwrap_or_else(|| ("a tool".to_string(), String::new()));
        let error = take_chars(&crate::ai::redaction::redact_text(&m.content), 200);
        let dedup_key = format!("{}\u{0}{}", tool, first_line(&error).to_lowercase());
        if seen.insert(dedup_key) {
            out.failures.push(ToolFailure { tool, args_brief, error });
        }
    }

    out.hit_max_iters = max_iters > 0 && iters_used >= max_iters;

    // Capability gaps: the agent's prose professed ignorance / declined. Capture the
    // user request that prompted it so the next turn can research it with learn_skill.
    let mut last_user = String::new();
    let mut gap_seen: HashSet<String> = HashSet::new();
    for m in messages {
        if m.role == "user" {
            last_user = first_line(&m.content).to_string();
        } else if m.role == "assistant" && !m.content.trim().is_empty() {
            let lc = m.content.to_lowercase();
            if IGNORANCE_PHRASES.iter().any(|p| lc.contains(p)) {
                let topic = take_chars(&last_user, 80);
                if !topic.is_empty() && gap_seen.insert(topic.to_lowercase()) {
                    out.gaps.push(topic);
                }
            }
        }
    }
    out
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s).trim()
}

/// Map a common error to a one-clause corrective hint (heuristic, no LLM).
fn failure_hint(error: &str) -> &'static str {
    let e = error.to_lowercase();
    if e.contains("command not found") || e.contains("not found in path") || e.contains("no such command") {
        " The command/binary wasn't available — check it's installed or use an alternative."
    } else if e.contains("permission denied") || e.contains("operation not permitted") {
        " Permission was denied — use a writable path (e.g. /root or /tmp on a Linux VPS as root) or elevate."
    } else if e.contains("no such file") || e.contains("not a directory") || e.contains("does not exist") {
        " The path didn't exist — verify it first by listing/reading before acting on it."
    } else if e.contains("timed out") || e.contains("timeout") {
        " It timed out — prefer a lighter, more targeted command."
    } else if e.contains("connection refused") || e.contains("could not connect") || e.contains("connection reset") {
        " The connection failed — confirm the host/port and that the service is running."
    } else {
        ""
    }
}

/// Turn an outcome into short, first-person lessons to remember. Pure.
pub fn distill_lessons(outcome: &TurnOutcome) -> Vec<String> {
    let mut lessons = Vec::new();
    for f in &outcome.failures {
        let where_ = if f.args_brief.is_empty() {
            String::new()
        } else {
            format!(" (called with: {})", f.args_brief)
        };
        lessons.push(format!(
            "{LESSON_TAG} `{}`{where_} failed with \"{}\".{}",
            f.tool,
            first_line(&f.error),
            failure_hint(&f.error)
        ));
    }
    for t in &outcome.repeated_tools {
        lessons.push(format!(
            "{LESSON_TAG} I called `{t}` repeatedly with the same arguments in one task — when a result doesn't change, change the approach instead of retrying the identical call."
        ));
    }
    if outcome.hit_max_iters {
        lessons.push(format!(
            "{LESSON_TAG} A task hit the tool-iteration limit without finishing — break large tasks into smaller, individually-verified steps rather than looping."
        ));
    }
    for g in &outcome.gaps {
        lessons.push(format!(
            "{GAP_TAG} I told the user I couldn't help with \"{g}\". Next time, call learn_skill to \
             research it and build a skill before answering, instead of declining."
        ));
    }
    lessons
}

fn normalize(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `existing` memory already contains a substantially-similar lesson, so we
/// don't append duplicates every time the same mistake recurs.
fn is_duplicate(existing_normalized: &str, lesson: &str) -> bool {
    let n = normalize(lesson);
    // Compare on a stable prefix (tool + error gist) — enough to catch repeats while
    // tolerating tiny wording differences.
    let key: String = n.chars().take(50).collect();
    !key.is_empty() && existing_normalized.contains(&key)
}

/// The full self-improvement step, called once at the end of a turn. Analyzes the
/// transcript, distills lessons, and appends the new (non-duplicate) ones to memory.
/// Returns the lessons actually saved (empty when the turn went fine). Side-effect:
/// writes to `home`'s MEMORY.md via [`memory::append_memory`].
pub fn reflect_and_save(
    home: &AgentHome,
    messages: &[ChatMessage],
    iters_used: usize,
    max_iters: usize,
) -> Vec<String> {
    let outcome = analyze_turn(messages, iters_used, max_iters);
    if !outcome.had_trouble() {
        return Vec::new();
    }
    let existing_normalized = normalize(&memory::load_memory(home));
    let mut saved = Vec::new();
    let mut running = existing_normalized;
    for lesson in distill_lessons(&outcome) {
        if is_duplicate(&running, &lesson) {
            continue;
        }
        if memory::append_memory(home, &lesson).is_ok() {
            running.push(' ');
            running.push_str(&normalize(&lesson));
            saved.push(lesson);
        }
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::ToolCall;
    use serde_json::json;

    fn assistant_call(id: &str, name: &str, args: serde_json::Value) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: vec![ToolCall { id: id.into(), name: name.into(), arguments: args }],
            tool_call_id: None,
        }
    }
    fn tool_result(id: &str, content: &str) -> ChatMessage {
        ChatMessage::tool_result(id, content)
    }

    #[test]
    fn clean_turn_yields_no_lessons() {
        let msgs = vec![
            ChatMessage::user("hi"),
            ChatMessage::assistant("hello!"),
        ];
        let outcome = analyze_turn(&msgs, 1, 12);
        assert!(!outcome.had_trouble());
        assert!(distill_lessons(&outcome).is_empty());
    }

    #[test]
    fn detects_failed_tool_and_maps_name() {
        let msgs = vec![
            ChatMessage::user("run foo"),
            assistant_call("t1", "run_command", json!({"command": "foo"})),
            tool_result("t1", "error: bash: foo: command not found"),
        ];
        let outcome = analyze_turn(&msgs, 1, 12);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].tool, "run_command");
        let lessons = distill_lessons(&outcome);
        assert_eq!(lessons.len(), 1);
        assert!(lessons[0].contains("run_command"));
        assert!(lessons[0].contains("command not found"));
        assert!(lessons[0].to_lowercase().contains("alternative")); // hint applied
    }

    #[test]
    fn a_lesson_never_carries_argument_values() {
        // These lessons are written to agent/MEMORY.md and re-sent to the provider on
        // every later turn, so a credential in a failed call must not reach them.
        let msgs = vec![
            ChatMessage::user("check the db"),
            assistant_call(
                "t1",
                "local_run_command",
                json!({"command": "mysql -u root -pHUNTER2 -e 'select 1'"}),
            ),
            tool_result("t1", "error: ERROR 1045 (28000): Access denied"),
        ];
        let outcome = analyze_turn(&msgs, 1, 12);
        let lessons = distill_lessons(&outcome);
        assert_eq!(lessons.len(), 1);
        assert!(!lessons[0].contains("HUNTER2"), "leaked a password: {}", lessons[0]);
        // Not just the secret — none of the command text should be there, since a value
        // anywhere in it could be a credential.
        assert!(!lessons[0].contains("mysql"), "leaked the command: {}", lessons[0]);
        // Still useful: names the tool, the argument shape, and the error.
        assert!(lessons[0].contains("local_run_command"), "{}", lessons[0]);
        assert!(lessons[0].contains("called with: command"), "{}", lessons[0]);
        assert!(lessons[0].contains("Access denied"), "{}", lessons[0]);
    }

    #[test]
    fn arg_shape_lists_keys_only() {
        assert_eq!(arg_shape(&json!({"path": "/root/.ssh/id_rsa"})), "path");
        // Order follows serde_json's map, which preserves insertion order only with the
        // preserve_order feature; assert on content rather than exact ordering.
        let shape = arg_shape(&json!({"host": "h", "token": "sekrit"}));
        assert!(shape.contains("host") && shape.contains("token"), "{shape}");
        assert!(!shape.contains("sekrit"), "{shape}");
        // Non-objects and empty objects contribute nothing.
        assert_eq!(arg_shape(&json!({})), "");
        assert_eq!(arg_shape(&json!("just a string")), "");
        assert_eq!(arg_shape(&json!(null)), "");
    }

    #[test]
    fn detects_retry_thrashing() {
        let msgs = vec![
            assistant_call("t1", "read_file", json!({"path": "/x"})),
            tool_result("t1", "ok: contents"),
            assistant_call("t2", "read_file", json!({"path": "/x"})),
            tool_result("t2", "ok: contents"),
        ];
        let outcome = analyze_turn(&msgs, 2, 12);
        assert_eq!(outcome.repeated_tools, vec!["read_file"]);
        assert!(outcome.had_trouble());
    }

    #[test]
    fn detects_max_iters() {
        let msgs = vec![ChatMessage::user("big task")];
        let outcome = analyze_turn(&msgs, 12, 12);
        assert!(outcome.hit_max_iters);
        assert!(distill_lessons(&outcome).iter().any(|l| l.contains("iteration limit")));
    }

    #[test]
    fn detects_capability_gap_and_primes_learn_skill() {
        let msgs = vec![
            ChatMessage::user("set up a wireguard vpn on my server"),
            ChatMessage::assistant("Sorry, I don't know how to configure WireGuard."),
        ];
        let outcome = analyze_turn(&msgs, 1, 12);
        assert_eq!(outcome.gaps.len(), 1);
        assert!(outcome.had_trouble());
        let lessons = distill_lessons(&outcome);
        assert!(lessons.iter().any(|l| l.contains("[gap]") && l.contains("learn_skill")));
    }

    #[test]
    fn clean_action_turn_has_no_gap() {
        let msgs = vec![
            ChatMessage::user("restart nginx"),
            ChatMessage::assistant("Done — nginx restarted."),
        ];
        assert!(analyze_turn(&msgs, 1, 12).gaps.is_empty());
    }

    #[test]
    fn dedup_prevents_repeat_lessons() {
        let lesson = format!("{LESSON_TAG} `run_command` failed with \"error: x\".");
        let existing = normalize(&lesson);
        assert!(is_duplicate(&existing, &lesson));
        assert!(!is_duplicate("", &lesson));
    }

    #[test]
    fn reflect_and_save_writes_then_dedupes() {
        let dir = std::env::temp_dir().join(format!("xc-reflect-test-{}", std::process::id()));
        let home = AgentHome::new(dir.clone());
        let msgs = vec![
            assistant_call("t1", "run_command", json!({"command": "foo"})),
            tool_result("t1", "error: foo: command not found"),
        ];
        let first = reflect_and_save(&home, &msgs, 1, 12);
        assert_eq!(first.len(), 1, "first reflection should save one lesson");
        let second = reflect_and_save(&home, &msgs, 1, 12);
        assert!(second.is_empty(), "identical mistake should not be saved twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reflection_redacts_secret_bearing_tool_errors() {
        let dir = std::env::temp_dir().join(format!("xc-reflect-secret-{}", std::process::id()));
        let home = AgentHome::new(dir.clone());
        let msgs = vec![
            assistant_call("t1", "run_command", json!({"command": "mysql"})),
            tool_result(
                "t1",
                "error: command failed: mysql -u root -pHUNTER2 --token sk-test-abc123",
            ),
        ];
        let saved = reflect_and_save(&home, &msgs, 1, 12);
        let memory = memory::load_memory(&home);
        assert_eq!(saved.len(), 1);
        assert!(!memory.contains("HUNTER2"));
        assert!(!memory.contains("sk-test-abc123"));
        assert!(!memory.contains("mysql -u root"));
        assert!(memory.contains("run_command"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reflection_preserves_safe_failure_learning() {
        let dir = std::env::temp_dir().join(format!("xc-reflect-safe-{}", std::process::id()));
        let home = AgentHome::new(dir.clone());
        let msgs = vec![
            assistant_call("t1", "run_command", json!({"command": "systemctl"})),
            tool_result("t1", "error: command not found: systemctl"),
        ];
        reflect_and_save(&home, &msgs, 1, 12);
        let memory = memory::load_memory(&home);
        assert!(memory.contains("command not found"));
        assert!(memory.contains("installed") || memory.contains("alternative"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reflection_redacts_other_secret_shapes_and_dedupes() {
        let dir = std::env::temp_dir().join(format!("xc-reflect-shapes-{}", std::process::id()));
        let home = AgentHome::new(dir.clone());
        let msgs = vec![
            assistant_call("t1", "run_command", json!({"command": "curl"})),
            tool_result(
                "t1",
                "error: Authorization: Bearer fixture-token postgres://admin:fixture-password@example.invalid/db {\"password\":\"fixture-password\"}",
            ),
        ];
        let first = reflect_and_save(&home, &msgs, 1, 12);
        let second = reflect_and_save(&home, &msgs, 1, 12);
        let memory = memory::load_memory(&home);
        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert!(!memory.contains("fixture-token"));
        assert!(!memory.contains("fixture-password"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
