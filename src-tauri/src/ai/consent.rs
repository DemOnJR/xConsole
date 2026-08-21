//! Classify chat replies that should resolve a waiting interactive step
//! (plan review, ask_user, command approval) instead of starting a new
//! request that then sits idle.
//!
//! The model can write a plan as ordinary assistant text and skip
//! `present_plan`. The user then types "ok the plan looks good" in chat.
//! Without this module that message is just another user turn, plan mode
//! still blocks mutations, and the agent stops.

use crate::ai::provider::{ChatMessage, ToolCall};
use serde_json::json;

/// What a short chat reply means while the agent is waiting on the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatIntent {
    /// User accepted the waiting plan / command.
    Approve,
    /// User wants the plan revised. `feedback` is the original text (or the
    /// remainder after a reject prefix).
    Reject { feedback: String },
    /// User cancelled the waiting step.
    Cancel,
    /// "continue" / "keep going" — resume the outstanding plan or turn.
    Continue,
    /// Not a decision. Treat as a new request (or as an ask_user answer).
    Other,
}

/// Last real user turn, skipping the trailing `# Runtime context` block.
pub fn last_user_text(messages: &[ChatMessage]) -> &str {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "user" && !crate::ai::context::is_runtime_message(m))
        .map(|m| m.content.as_str())
        .unwrap_or("")
}

/// Last real assistant turn (not a tool-result row).
pub fn last_assistant_text(messages: &[ChatMessage]) -> &str {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| m.content.as_str())
        .unwrap_or("")
}

/// Classify a user chat line. Conservative: only short, decision-shaped
/// phrases count as Approve / Reject / Cancel / Continue. "ok make a plan
/// for the decoy" stays `Other`.
pub fn classify_chat(text: &str) -> ChatIntent {
    let raw = text.trim();
    if raw.is_empty() {
        return ChatIntent::Other;
    }
    let t = normalize(raw);

    if is_exact(&t, CANCEL) {
        return ChatIntent::Cancel;
    }
    if is_exact(&t, REJECT) {
        return ChatIntent::Reject {
            feedback: raw.to_string(),
        };
    }
    if let Some(rest) = strip_prefix_list(&t, REJECT_PREFIX) {
        let feedback = if rest.is_empty() {
            raw.to_string()
        } else {
            rest.to_string()
        };
        return ChatIntent::Reject { feedback };
    }
    if is_exact(&t, CONTINUE) {
        return ChatIntent::Continue;
    }
    if is_exact(&t, APPROVE) {
        return ChatIntent::Approve;
    }
    if mentions_plan_approval(&t) && !looks_like_new_task(&t) {
        return ChatIntent::Approve;
    }
    if let Some(rest) = strip_prefix_list(&t, APPROVE_PREFIX) {
        if rest.is_empty() {
            return ChatIntent::Approve;
        }
        // "ok looks good, also change the ssh port" — approve + extra notes.
        if mentions_plan(&t) || !looks_like_new_task(rest) {
            return ChatIntent::Approve;
        }
        // "ok make a honeypot" is a new request that happens to start with ok.
        return ChatIntent::Other;
    }
    ChatIntent::Other
}

/// True when this chat line should lift the plan-mode mutation guard and
/// tell the agent to execute.
pub fn chat_approves_plan(text: &str, previous_assistant: &str) -> bool {
    match classify_chat(text) {
        ChatIntent::Approve => true,
        ChatIntent::Continue => looks_like_plan(previous_assistant),
        _ => false,
    }
}

/// True when this chat line is a revision / rejection of the outstanding plan.
pub fn chat_rejects_plan(text: &str) -> bool {
    matches!(classify_chat(text), ChatIntent::Reject { .. } | ChatIntent::Cancel)
}

/// Assistant text that is a real plan (numbered steps / a Plan heading),
/// not a one-line "I'll write a plan next".
pub fn looks_like_plan(text: &str) -> bool {
    let t = text.trim();
    if t.len() < 40 {
        return false;
    }
    let steps = count_numbered_steps(t);
    if steps >= 2 {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    let heading = lower.lines().any(|line| {
        let s = line.trim();
        (s.starts_with("## ") || s.starts_with("# ") || s.starts_with("### "))
            && s.contains("plan")
    });
    if heading && t.len() >= 160 {
        return true;
    }
    if steps >= 1 && t.len() >= 160 && (lower.contains("approve") || heading) {
        return true;
    }
    false
}

/// Title for a chat-text plan: first markdown heading, else "Plan".
pub fn plan_title(text: &str) -> String {
    for line in text.lines() {
        let s = line.trim();
        let rest = s
            .strip_prefix("### ")
            .or_else(|| s.strip_prefix("## "))
            .or_else(|| s.strip_prefix("# "));
        if let Some(title) = rest {
            let title = title.trim();
            if !title.is_empty() {
                return title.chars().take(80).collect();
            }
        }
    }
    "Plan".into()
}

/// The `plan` argument under any name the model commonly uses.
pub fn plan_body_from_args(args: &serde_json::Value) -> Option<String> {
    const KEYS: &[&str] = &["plan", "content", "text", "steps", "markdown", "body"];
    for key in KEYS {
        if let Some(s) = args.get(*key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
        if let Some(arr) = args.get(*key).and_then(|v| v.as_array()) {
            let lines: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !lines.is_empty() {
                let body = lines
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("{}. {s}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some(body);
            }
        }
    }
    None
}

/// If plan mode is on, nothing is approved yet, and the model wrote a plan
/// as chat text, synthesize a `present_plan` call so the review modal opens.
pub fn synthetic_present_plan(
    plan_mode: bool,
    approved: bool,
    assistant_text: &str,
) -> Option<ToolCall> {
    if !plan_mode || approved {
        return None;
    }
    if !looks_like_plan(assistant_text) {
        return None;
    }
    Some(ToolCall {
        id: format!("present_plan_{}", uuid::Uuid::new_v4()),
        name: "present_plan".into(),
        arguments: json!({
            "title": plan_title(assistant_text),
            "plan": assistant_text,
        }),
    })
}

/// After the user approved, an empty or "waiting for you" reply should be
/// nudged once so the model actually starts executing.
pub fn should_nudge_execute(
    plan_mode: bool,
    approved: bool,
    assistant_text: &str,
    already_nudged: bool,
) -> bool {
    if already_nudged || !plan_mode || !approved {
        return false;
    }
    let t = assistant_text.trim();
    t.is_empty() || is_waiting_ack(t)
}

fn is_waiting_ack(text: &str) -> bool {
    let t = normalize(text);
    if t.len() > 220 {
        return false;
    }
    WAITING.iter().any(|p| t.contains(p))
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
            continue;
        }
        prev_space = false;
        // Keep letters/digits; drop wrapping punctuation.
        if c.is_ascii_alphanumeric() || c == '\'' {
            out.push(c.to_ascii_lowercase());
        } else if c == '-' {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn is_exact(t: &str, list: &[&str]) -> bool {
    list.iter().any(|p| t == *p)
}

fn strip_prefix_list<'a>(t: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let mut best: Option<&str> = None;
    for p in prefixes {
        if let Some(rest) = t.strip_prefix(p) {
            let rest = rest.trim_start();
            if best.map(|cur| rest.len() < cur.len()).unwrap_or(true) {
                best = Some(rest);
            }
        }
    }
    best
}

fn mentions_plan(t: &str) -> bool {
    t.contains("plan") || t.contains("steps")
}

fn mentions_plan_approval(t: &str) -> bool {
    mentions_plan(t)
        && APPROVE_HINT
            .iter()
            .any(|h| t.contains(h))
        && !looks_like_new_task(t)
}

fn looks_like_new_task(t: &str) -> bool {
    NEW_TASK.iter().any(|v| {
        t.starts_with(v)
            || t.contains(&format!(" {v} "))
            || t.contains(&format!(" {v}"))
    })
}

fn count_numbered_steps(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let s = line.trim_start();
            let mut digits = 0usize;
            for b in s.bytes() {
                if b.is_ascii_digit() {
                    digits += 1;
                } else {
                    return digits > 0 && (b == b'.' || b == b')');
                }
            }
            false
        })
        .count()
}

const APPROVE: &[&str] = &[
    "ok",
    "okay",
    "k",
    "yes",
    "y",
    "yep",
    "yeah",
    "yas",
    "sure",
    "ok sure",
    "yes please",
    "yes do it",
    "approve",
    "approved",
    "i approve",
    "lgtm",
    "looks good",
    "looks great",
    "looks fine",
    "sounds good",
    "sounds great",
    "perfect",
    "great",
    "good",
    "go",
    "go ahead",
    "go for it",
    "do it",
    "do it now",
    "just do it",
    "ship it",
    "ship",
    "execute",
    "execute it",
    "run it",
    "run them",
    "apply",
    "apply it",
    "apply and run",
    "proceed",
    "please proceed",
    "lets go",
    "let's go",
    "lets do it",
    "let's do it",
    "ok go",
    "ok do it",
    "ok proceed",
    "ok execute",
    "ok apply",
    "ok run it",
    "ok looks good",
    "okay looks good",
    "ok the plan looks good",
    "okay the plan looks good",
    "the plan looks good",
    "plan looks good",
    "looks good to me",
    "thats fine",
    "that's fine",
    "thats good",
    "that's good",
    "all good",
    "good to go",
];

const APPROVE_PREFIX: &[&str] = &[
    "ok the plan looks good",
    "okay the plan looks good",
    "ok looks good",
    "okay looks good",
    "looks good",
    "lgtm",
    "approved",
    "approve",
    "go ahead",
    "sounds good",
    "ok go",
    "okay go",
];

const APPROVE_HINT: &[&str] = &[
    "looks good",
    "lgtm",
    "approve",
    "approved",
    "go ahead",
    "do it",
    "ship it",
    "sounds good",
    "good to go",
];

const REJECT: &[&str] = &[
    "no",
    "nope",
    "nah",
    "reject",
    "rejected",
    "deny",
    "denied",
    "dont",
    "don't",
    "do not",
    "no thanks",
    "not yet",
    "not now",
    "hold on",
    "wait",
    "stop",
];

const REJECT_PREFIX: &[&str] = &[
    "reject",
    "rejected",
    "no ",
    "nope ",
    "dont ",
    "don't ",
    "do not ",
    "change ",
    "revise ",
    "instead ",
];

const CANCEL: &[&str] = &[
    "cancel",
    "cancelled",
    "canceled",
    "nevermind",
    "never mind",
    "forget it",
    "abort",
];

const CONTINUE: &[&str] = &[
    "continue",
    "keep going",
    "keep on",
    "go on",
    "resume",
    "carry on",
    "dont stop",
    "don't stop",
];

const WAITING: &[&str] = &[
    "waiting for",
    "let me know",
    "approve to",
    "once you approve",
    "when you approve",
    "awaiting",
    "say the word",
    "want me to proceed",
    "shall i",
];

const NEW_TASK: &[&str] = &[
    "make ",
    "make a",
    "make an",
    "create ",
    "add ",
    "build ",
    "write ",
    "implement ",
    "fix ",
    "check ",
    "investigate ",
    "plan ",
    "draft ",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::ChatMessage;

    #[test]
    fn exact_user_phrase_approves() {
        assert_eq!(classify_chat("ok the plan looks good"), ChatIntent::Approve);
        assert_eq!(classify_chat("ok looks good"), ChatIntent::Approve);
        assert_eq!(classify_chat("  OK the plan looks good.  "), ChatIntent::Approve);
        assert_eq!(classify_chat("lgtm"), ChatIntent::Approve);
        assert_eq!(classify_chat("go ahead"), ChatIntent::Approve);
        assert_eq!(classify_chat("yes"), ChatIntent::Approve);
        assert_eq!(classify_chat("apply and run"), ChatIntent::Approve);
    }

    #[test]
    fn new_request_starting_with_ok_is_not_approval() {
        assert_eq!(
            classify_chat("ok make an plan for the decoy"),
            ChatIntent::Other
        );
        assert_eq!(
            classify_chat("ok create a firewall for those bruteforce attacks"),
            ChatIntent::Other
        );
    }

    #[test]
    fn reject_and_cancel() {
        assert!(matches!(classify_chat("no"), ChatIntent::Reject { .. }));
        assert!(matches!(
            classify_chat("change the ssh port first"),
            ChatIntent::Reject { .. }
        ));
        assert_eq!(classify_chat("cancel"), ChatIntent::Cancel);
        assert_eq!(classify_chat("continue"), ChatIntent::Continue);
    }

    #[test]
    fn chat_approves_plan_continue_only_after_a_plan() {
        let plan = "## Plan: honeypot on port 22\n\n1. Install Cowrie in a venv on each host\n\
                    2. Wire fail2ban to ban on first touch\n3. Verify real SSH on 2222 still works\n";
        assert!(chat_approves_plan("ok the plan looks good", plan));
        assert!(chat_approves_plan("continue", plan));
        assert!(!chat_approves_plan("continue", "Sure, I can take a look."));
        assert!(!chat_approves_plan("ok make an plan for the decoy", plan));
    }

    #[test]
    fn looks_like_plan_detects_numbered_hardening() {
        let plan = "\
## Plan: SSH honeypot + auto-ban on port 22 (both hosts)\n\
\n\
**Design:** Any connection to port 22 is an attacker.\n\
\n\
1. **Install Cowrie** in a Python venv on each host.\n\
2. **Configure it** to listen on 0.0.0.0:22.\n\
3. **systemd service** cowrie.service.\n\
4. **fail2ban jail** maxretry = 1.\n\
5. **Verify** end-to-end.\n\
6. **Confirm** real SSH on 2222 is untouched.\n\
\n\
Approve and I'll build it on both hosts.";
        assert!(looks_like_plan(plan));
        assert_eq!(plan_title(plan), "Plan: SSH honeypot + auto-ban on port 22 (both hosts)");
        assert!(!looks_like_plan("I will write a plan next."));
        assert!(!looks_like_plan("ok"));
    }

    #[test]
    fn plan_body_accepts_aliases_the_model_uses() {
        assert_eq!(
            plan_body_from_args(&json!({"plan": "  do x  "})).as_deref(),
            Some("do x")
        );
        assert_eq!(
            plan_body_from_args(&json!({"content": "do x"})).as_deref(),
            Some("do x")
        );
        assert_eq!(
            plan_body_from_args(&json!({"steps": ["install cowrie", "wire fail2ban"]}))
                .as_deref(),
            Some("1. install cowrie\n2. wire fail2ban")
        );
        assert!(plan_body_from_args(&json!({"title": "only title"})).is_none());
    }

    #[test]
    fn last_user_skips_runtime_context() {
        let msgs = vec![
            ChatMessage::user("ok the plan looks good"),
            ChatMessage::user("# Runtime context\n# Live canvas\n"),
        ];
        assert_eq!(last_user_text(&msgs), "ok the plan looks good");
    }

    #[test]
    fn empty_approved_reply_is_nudged_once() {
        assert!(should_nudge_execute(true, true, "", false));
        assert!(should_nudge_execute(
            true,
            true,
            "Waiting for you to approve.",
            false
        ));
        assert!(!should_nudge_execute(true, true, "", true));
        assert!(!should_nudge_execute(true, false, "", false));
        assert!(!should_nudge_execute(false, true, "", false));
    }

    #[test]
    fn synthetic_present_plan_only_in_unapproved_plan_mode() {
        let plan = "## Plan: decoy\n\n1. Install Cowrie\n2. Wire fail2ban\n3. Verify port 2222\n";
        assert!(synthetic_present_plan(true, false, plan).is_some());
        assert!(synthetic_present_plan(true, true, plan).is_none());
        assert!(synthetic_present_plan(false, false, plan).is_none());
        assert!(synthetic_present_plan(true, false, "I'll write a plan next.").is_none());
    }
}
