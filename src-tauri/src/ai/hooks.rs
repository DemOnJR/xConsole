//! Claude Code–style hooks: user-defined shell commands that fire on agent
//! lifecycle events. The same model Claude Code uses — a JSON config maps an
//! event (and, for tool events, a tool-name matcher) to one or more shell
//! commands. Each command receives the event payload as JSON on **stdin** and
//! controls the agent through its **exit code** and/or a JSON object on **stdout**.
//!
//! Events wired into xConsole's agent loop:
//! - `UserPromptSubmit` — before the turn runs (can inject extra context, or block).
//! - `PreToolUse`        — before a tool runs (can block the tool, or add context).
//! - `PostToolUse`       — after a tool runs (can feed the result back / add context).
//! - `Stop`              — after the turn finishes (side-effects, notifications).
//!
//! Decision protocol (mirrors Claude Code):
//! - exit `0`  → success. For `UserPromptSubmit`, plain stdout is injected as context.
//! - exit `2`  → blocking error. The tool/prompt is blocked; stderr is the reason.
//! - other     → non-blocking error (logged, the agent proceeds).
//! - A JSON object on stdout is the "advanced" path: `{"decision":"block","reason":…}`,
//!   `{"continue":false}`, `{"hookSpecificOutput":{"permissionDecision":"deny"|"allow",
//!   "additionalContext":…}}`, `systemMessage`, …
//!
//! Config is read from `hooks.json` in the agent home and **snapshotted at startup**
//! (managed [`HooksState`]) — exactly like Claude Code, so a mid-session edit (including
//! one the agent itself might write) does not take effect until an explicit reload.
//!
//! The config parsing, matcher matching, and output interpretation are PURE functions
//! (no I/O) so they're deterministic and unit-testable; only [`run_one`] spawns a process.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::AgentHome;

/// File in the agent home that holds the hooks config (Claude Code's `settings.json`
/// `hooks` block, standalone).
pub const HOOKS_FILE: &str = "hooks.json";

/// Default per-hook timeout when the config doesn't set one. Clamped to [1, 600].
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 600;

/// The lifecycle events a hook can subscribe to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
    Stop,
    Notification,
    SessionStart,
    SessionEnd,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::Stop => "Stop",
            HookEvent::Notification => "Notification",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SessionEnd => "SessionEnd",
        }
    }

    /// Parse an event name from the config. Unknown keys are ignored (so the config
    /// can hold forward-compatible keys without erroring).
    pub fn from_str(s: &str) -> Option<HookEvent> {
        Some(match s {
            "PreToolUse" => HookEvent::PreToolUse,
            "PostToolUse" => HookEvent::PostToolUse,
            "UserPromptSubmit" => HookEvent::UserPromptSubmit,
            "Stop" => HookEvent::Stop,
            "Notification" => HookEvent::Notification,
            "SessionStart" => HookEvent::SessionStart,
            "SessionEnd" => HookEvent::SessionEnd,
            _ => return None,
        })
    }

    /// Whether this event is tool-scoped (its matcher selects on a tool name).
    fn is_tool_event(self) -> bool {
        matches!(self, HookEvent::PreToolUse | HookEvent::PostToolUse)
    }
}

fn default_kind() -> String {
    "command".to_string()
}

/// One shell command to run for a matched event.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HookCommand {
    /// Only `"command"` is supported (matches Claude Code's hook type).
    #[serde(rename = "type", default = "default_kind")]
    pub kind: String,
    /// The shell command line, run via `cmd /C` on Windows or `sh -c` elsewhere.
    pub command: String,
    /// Optional per-hook timeout in seconds.
    #[serde(default)]
    pub timeout: Option<u64>,
}

/// A matcher group: a tool-name pattern plus the commands to run when it matches.
/// For non-tool events the matcher is ignored (the commands always run).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HookMatcher {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookCommand>,
}

/// The parsed hooks configuration: event name → its matcher groups.
#[derive(Clone, Debug, Default)]
pub struct HooksConfig {
    events: BTreeMap<String, Vec<HookMatcher>>,
}

impl HooksConfig {
    /// Parse a `hooks.json` document. Accepts either the wrapped form
    /// `{"hooks":{"PreToolUse":[…]}}` (Claude Code's settings.json shape) or the bare
    /// `{"PreToolUse":[…]}`. Unknown event keys are ignored. Pure.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("hooks.json is not valid JSON: {e}"))?;
        // Unwrap an optional top-level "hooks" key.
        let root = v.get("hooks").unwrap_or(&v);
        let obj = root
            .as_object()
            .ok_or_else(|| "hooks config must be a JSON object".to_string())?;

        let mut events = BTreeMap::new();
        for (key, val) in obj {
            let Some(event) = HookEvent::from_str(key) else {
                continue; // forward-compatible: skip keys we don't know
            };
            let matchers: Vec<HookMatcher> = serde_json::from_value(val.clone())
                .map_err(|e| format!("hooks.{key} is malformed: {e}"))?;
            events.insert(event.as_str().to_string(), matchers);
        }
        Ok(Self { events })
    }

    /// Load the snapshot from `hooks.json` in the agent home. A missing or empty file
    /// yields an empty config (hooks are opt-in). A malformed file is ignored with a
    /// log line rather than failing the app.
    pub fn load(home: &AgentHome) -> Self {
        let path = home.0.join(HOOKS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => Self::parse(&text).unwrap_or_else(|e| {
                eprintln!("hooks: ignoring {}: {e}", path.display());
                Self::default()
            }),
            _ => Self::default(),
        }
    }

    /// Strict load used by the save/reload command path — surfaces parse errors so the
    /// UI can show them.
    pub fn load_strict(home: &AgentHome) -> Result<Self, String> {
        let path = home.0.join(HOOKS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => Self::parse(&text),
            _ => Ok(Self::default()),
        }
    }

    /// Total number of hook commands across every event (for status display).
    pub fn total(&self) -> usize {
        self.events
            .values()
            .flat_map(|ms| ms.iter())
            .map(|m| m.hooks.len())
            .sum()
    }

    /// Number of hook commands subscribed to a single event.
    pub fn count(&self, event: HookEvent) -> usize {
        self.events
            .get(event.as_str())
            .map(|ms| ms.iter().map(|m| m.hooks.len()).sum())
            .unwrap_or(0)
    }

    /// Whether any command is configured for `event`.
    pub fn has_event(&self, event: HookEvent) -> bool {
        self.count(event) > 0
    }

    /// The commands that should fire for `event` (and, for tool events, `tool`).
    /// Pure — this is what the runner iterates over.
    pub fn select(&self, event: HookEvent, tool: Option<&str>) -> Vec<HookCommand> {
        let mut out = Vec::new();
        if let Some(matchers) = self.events.get(event.as_str()) {
            for m in matchers {
                let tool_for_match = if event.is_tool_event() { tool } else { None };
                if matcher_matches(m.matcher.as_deref(), tool_for_match) {
                    out.extend(m.hooks.iter().cloned());
                }
            }
        }
        out
    }
}

/// Whether a matcher pattern selects a tool. Empty / `None` / `"*"` matches everything.
/// Otherwise the pattern is a `|`-separated list of exact tool names — Claude Code's
/// common matcher form, e.g. `"write_file|run_command"`. (Full regex isn't supported to
/// avoid a new dependency; alternation + wildcard covers the practical cases.) Pure.
pub fn matcher_matches(pattern: Option<&str>, tool: Option<&str>) -> bool {
    let p = pattern.unwrap_or("").trim();
    if p.is_empty() || p == "*" {
        return true;
    }
    // Non-tool events carry no tool name; a present matcher there still matches.
    let Some(tool) = tool else {
        return true;
    };
    p.split('|').map(str::trim).any(|name| name == tool)
}

// ---- Event input (stdin payload) ----------------------------------------

/// The JSON payload handed to a hook on stdin. Field set mirrors Claude Code's
/// (`session_id`, `cwd`, `hook_event_name`, `tool_name`, `tool_input`, `tool_response`,
/// `prompt`) plus xConsole context (`workspace_id`, `vps_targets`).
pub struct HookEventInput<'a> {
    pub event: HookEvent,
    pub session_id: &'a str,
    pub cwd: &'a str,
    pub workspace_id: Option<&'a str>,
    pub vps_targets: &'a [String],
    pub tool_name: Option<&'a str>,
    pub tool_input: Option<&'a Value>,
    pub tool_response: Option<&'a str>,
    pub prompt: Option<&'a str>,
}

impl HookEventInput<'_> {
    pub fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("session_id".into(), json!(self.session_id));
        m.insert("cwd".into(), json!(self.cwd));
        m.insert("hook_event_name".into(), json!(self.event.as_str()));
        if let Some(w) = self.workspace_id.filter(|s| !s.is_empty()) {
            m.insert("workspace_id".into(), json!(w));
        }
        if !self.vps_targets.is_empty() {
            m.insert("vps_targets".into(), json!(self.vps_targets));
        }
        if let Some(t) = self.tool_name {
            m.insert("tool_name".into(), json!(t));
        }
        if let Some(i) = self.tool_input {
            m.insert("tool_input".into(), i.clone());
        }
        if let Some(r) = self.tool_response {
            m.insert("tool_response".into(), json!(r));
        }
        if let Some(p) = self.prompt {
            m.insert("prompt".into(), json!(p));
        }
        Value::Object(m)
    }
}

/// The current working directory string for a hook payload (best-effort).
pub fn cwd() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---- Decision (output interpretation) -----------------------------------

/// What a hook (or the merge of several) decided. The agent loop acts on this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HookDecision {
    /// The action should be blocked (tool not run / prompt rejected).
    pub block: bool,
    /// Halt the whole turn (`continue:false`).
    pub stop: bool,
    /// Reason shown to the model / user.
    pub reason: Option<String>,
    /// Extra context to inject (UserPromptSubmit / additionalContext).
    pub additional_context: Option<String>,
    /// A message to surface to the user (not the model).
    pub system_message: Option<String>,
    /// PreToolUse explicit permission: `"allow"` / `"deny"` / `"ask"`.
    pub permission: Option<String>,
}

impl HookDecision {
    /// Whether this decision blocks the action (explicit block or a deny permission).
    pub fn blocks(&self) -> bool {
        self.block || self.permission.as_deref() == Some("deny")
    }
}

/// Interpret ONE hook's result (exit code + stdout + stderr) for an event. Pure.
///
/// A JSON object on stdout is the structured path; otherwise exit-code semantics apply.
/// Exit code `2` is always a blocking error (with stderr as the reason).
pub fn parse_output(event: HookEvent, exit_code: i32, stdout: &str, stderr: &str) -> HookDecision {
    let mut d = HookDecision::default();
    let out = stdout.trim();

    let parsed = if out.starts_with('{') {
        serde_json::from_str::<Value>(out)
            .ok()
            .and_then(|v| v.as_object().cloned())
    } else {
        None
    };

    if let Some(o) = &parsed {
        if o.get("continue").and_then(Value::as_bool) == Some(false) {
            d.stop = true;
            d.block = true;
        }
        if let Some(s) = o.get("stopReason").and_then(Value::as_str) {
            d.reason = Some(s.to_string());
        }
        if let Some(s) = o.get("systemMessage").and_then(Value::as_str) {
            d.system_message = Some(s.to_string());
        }
        if o.get("decision").and_then(Value::as_str) == Some("block") {
            d.block = true;
        }
        if let Some(r) = o.get("reason").and_then(Value::as_str) {
            d.reason = Some(r.to_string());
        }
        if let Some(hso) = o.get("hookSpecificOutput").and_then(Value::as_object) {
            if let Some(c) = hso.get("additionalContext").and_then(Value::as_str) {
                d.additional_context = Some(c.to_string());
            }
            if let Some(pd) = hso.get("permissionDecision").and_then(Value::as_str) {
                d.permission = Some(pd.to_string());
                if pd == "deny" {
                    d.block = true;
                }
            }
            if let Some(pr) = hso.get("permissionDecisionReason").and_then(Value::as_str) {
                d.reason = Some(pr.to_string());
            }
        }
    } else if exit_code == 0
        && !out.is_empty()
        && matches!(event, HookEvent::UserPromptSubmit | HookEvent::SessionStart)
    {
        // Plain stdout from these events is injected as additional context.
        d.additional_context = Some(out.to_string());
    }

    // Exit-code floor: 2 is a blocking error regardless of stdout.
    if exit_code == 2 {
        d.block = true;
        if d.reason.is_none() {
            let e = stderr.trim();
            if !e.is_empty() {
                d.reason = Some(e.to_string());
            }
        }
    }
    d
}

/// Combine the decisions of every hook that fired for an event. Blocking wins; a `deny`
/// permission overrides an `allow`; reasons and contexts are concatenated. Pure.
pub fn merge(decisions: &[HookDecision]) -> HookDecision {
    let mut m = HookDecision::default();
    let mut reasons = Vec::new();
    let mut contexts = Vec::new();
    let mut messages = Vec::new();
    for d in decisions {
        m.block |= d.block;
        m.stop |= d.stop;
        match d.permission.as_deref() {
            Some("deny") => m.permission = Some("deny".into()),
            Some("allow") if m.permission.as_deref() != Some("deny") => {
                m.permission = Some("allow".into())
            }
            Some("ask") if m.permission.is_none() => m.permission = Some("ask".into()),
            _ => {}
        }
        if let Some(r) = &d.reason {
            reasons.push(r.clone());
        }
        if let Some(c) = &d.additional_context {
            contexts.push(c.clone());
        }
        if let Some(s) = &d.system_message {
            messages.push(s.clone());
        }
    }
    if !reasons.is_empty() {
        m.reason = Some(reasons.join("; "));
    }
    if !contexts.is_empty() {
        m.additional_context = Some(contexts.join("\n"));
    }
    if !messages.is_empty() {
        m.system_message = Some(messages.join("\n"));
    }
    m
}

// ---- Runner (the only I/O in this module) -------------------------------

struct RunResult {
    code: i32,
    stdout: String,
    stderr: String,
}

fn build_shell(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut c = crate::proc::quiet_tokio("cmd");
        c.arg("/C").arg(command);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = crate::proc::quiet_tokio("sh");
        c.arg("-c").arg(command);
        c
    }
}

/// Run a single hook command: spawn the shell, pipe `payload` to stdin, capture
/// stdout/stderr, and enforce the timeout (killing the child if it runs over).
async fn run_one(cmd: &HookCommand, payload: &str) -> RunResult {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let secs = cmd
        .timeout
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);

    let mut command = build_shell(&cmd.command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return RunResult {
                code: -1,
                stdout: String::new(),
                stderr: format!("hook spawn failed: {e}"),
            }
        }
    };

    // Feed the event payload, then close stdin so a reader sees EOF.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes()).await;
        // `stdin` drops here, closing the pipe.
    }

    match tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(out)) => RunResult {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Ok(Err(e)) => RunResult {
            code: -1,
            stdout: String::new(),
            stderr: format!("hook io error: {e}"),
        },
        // Timed out: the wait future is dropped, which (kill_on_drop) kills the child.
        Err(_) => RunResult {
            code: -1,
            stdout: String::new(),
            stderr: format!("hook timed out after {secs}s"),
        },
    }
}

/// Run every hook that matches `input`'s event (and tool), and merge their decisions.
/// Returns the default (no-op) decision when nothing matches.
pub async fn run_event(config: &HooksConfig, input: &HookEventInput<'_>) -> HookDecision {
    let cmds = config.select(input.event, input.tool_name);
    if cmds.is_empty() {
        return HookDecision::default();
    }
    let payload = input.to_json().to_string();
    let mut decisions = Vec::with_capacity(cmds.len());
    for c in &cmds {
        if c.kind != "command" || c.command.trim().is_empty() {
            continue;
        }
        let r = run_one(c, &payload).await;
        decisions.push(parse_output(input.event, r.code, &r.stdout, &r.stderr));
    }
    merge(&decisions)
}

// ---- Managed snapshot state ---------------------------------------------

/// The startup snapshot of the hooks config, shared as Tauri-managed state. Loaded once
/// at launch so mid-session edits to `hooks.json` (including ones the agent might write)
/// take effect only on an explicit reload — the same safety property Claude Code has.
#[derive(Clone)]
pub struct HooksState(Arc<RwLock<HooksConfig>>);

impl HooksState {
    pub fn new(config: HooksConfig) -> Self {
        Self(Arc::new(RwLock::new(config)))
    }

    /// A cheap clone of the current snapshot for one turn.
    pub fn snapshot(&self) -> HooksConfig {
        self.0.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Re-read `hooks.json` from disk and replace the snapshot. Returns the new hook
    /// count, or an error if the file is malformed (the snapshot is left unchanged).
    pub fn reload(&self, home: &AgentHome) -> Result<usize, String> {
        let config = HooksConfig::load_strict(home)?;
        let n = config.total();
        if let Ok(mut g) = self.0.write() {
            *g = config;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_wildcard_and_alternation() {
        assert!(matcher_matches(None, Some("run_command")));
        assert!(matcher_matches(Some(""), Some("run_command")));
        assert!(matcher_matches(Some("*"), Some("anything")));
        assert!(matcher_matches(Some("write_file|run_command"), Some("run_command")));
        assert!(!matcher_matches(Some("write_file|read_file"), Some("run_command")));
        // Non-tool events (tool = None) always match.
        assert!(matcher_matches(Some("write_file"), None));
    }

    #[test]
    fn parse_wrapped_and_bare_forms() {
        let wrapped = r#"{"hooks":{"PreToolUse":[{"matcher":"run_command","hooks":[{"type":"command","command":"echo hi"}]}]}}"#;
        let bare = r#"{"PreToolUse":[{"matcher":"run_command","hooks":[{"command":"echo hi"}]}]}"#;
        let a = HooksConfig::parse(wrapped).unwrap();
        let b = HooksConfig::parse(bare).unwrap();
        assert_eq!(a.count(HookEvent::PreToolUse), 1);
        assert_eq!(b.count(HookEvent::PreToolUse), 1);
        // `type` defaults to "command" when omitted (bare form).
        assert_eq!(b.select(HookEvent::PreToolUse, Some("run_command"))[0].kind, "command");
    }

    #[test]
    fn parse_ignores_unknown_events_and_rejects_garbage() {
        let cfg = HooksConfig::parse(r#"{"Bogus":[],"Stop":[{"hooks":[{"command":"x"}]}]}"#).unwrap();
        assert_eq!(cfg.count(HookEvent::Stop), 1);
        assert!(HooksConfig::parse("not json").is_err());
        assert!(HooksConfig::parse("[1,2,3]").is_err());
    }

    #[test]
    fn select_filters_by_tool_for_tool_events() {
        let cfg = HooksConfig::parse(
            r#"{"PreToolUse":[
                {"matcher":"run_command","hooks":[{"command":"a"}]},
                {"matcher":"write_file","hooks":[{"command":"b"}]},
                {"matcher":"*","hooks":[{"command":"c"}]}
            ]}"#,
        )
        .unwrap();
        let sel: Vec<String> = cfg
            .select(HookEvent::PreToolUse, Some("run_command"))
            .into_iter()
            .map(|c| c.command)
            .collect();
        assert_eq!(sel, vec!["a", "c"]); // run_command matcher + wildcard, not write_file
    }

    #[test]
    fn output_exit2_blocks_with_stderr_reason() {
        let d = parse_output(HookEvent::PreToolUse, 2, "", "nope, not allowed");
        assert!(d.blocks());
        assert_eq!(d.reason.as_deref(), Some("nope, not allowed"));
    }

    #[test]
    fn output_json_decision_block() {
        let d = parse_output(
            HookEvent::PreToolUse,
            0,
            r#"{"decision":"block","reason":"dangerous"}"#,
            "",
        );
        assert!(d.blocks());
        assert_eq!(d.reason.as_deref(), Some("dangerous"));
    }

    #[test]
    fn output_permission_deny_and_allow() {
        let deny = parse_output(
            HookEvent::PreToolUse,
            0,
            r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"blocked path"}}"#,
            "",
        );
        assert!(deny.blocks());
        assert_eq!(deny.reason.as_deref(), Some("blocked path"));

        let allow = parse_output(
            HookEvent::PreToolUse,
            0,
            r#"{"hookSpecificOutput":{"permissionDecision":"allow"}}"#,
            "",
        );
        assert!(!allow.blocks());
        assert_eq!(allow.permission.as_deref(), Some("allow"));
    }

    #[test]
    fn output_additional_context_paths() {
        // Plain stdout on UserPromptSubmit becomes context.
        let plain = parse_output(HookEvent::UserPromptSubmit, 0, "remember: prod is read-only", "");
        assert_eq!(
            plain.additional_context.as_deref(),
            Some("remember: prod is read-only")
        );
        // But plain stdout on PreToolUse is NOT context (only success output, ignored).
        let pre = parse_output(HookEvent::PreToolUse, 0, "some log line", "");
        assert!(pre.additional_context.is_none());
        // JSON additionalContext works on any event.
        let j = parse_output(
            HookEvent::PostToolUse,
            0,
            r#"{"hookSpecificOutput":{"additionalContext":"linted clean"}}"#,
            "",
        );
        assert_eq!(j.additional_context.as_deref(), Some("linted clean"));
    }

    #[test]
    fn output_continue_false_stops() {
        let d = parse_output(HookEvent::Stop, 0, r#"{"continue":false,"stopReason":"halt"}"#, "");
        assert!(d.stop);
        assert_eq!(d.reason.as_deref(), Some("halt"));
    }

    #[test]
    fn merge_block_wins_and_concatenates() {
        let a = HookDecision {
            additional_context: Some("ctx-a".into()),
            ..Default::default()
        };
        let b = HookDecision {
            block: true,
            reason: Some("bad".into()),
            permission: Some("deny".into()),
            additional_context: Some("ctx-b".into()),
            ..Default::default()
        };
        let c = HookDecision {
            permission: Some("allow".into()),
            ..Default::default()
        };
        let m = merge(&[a, b, c]);
        assert!(m.blocks());
        assert_eq!(m.permission.as_deref(), Some("deny")); // deny beats allow
        assert_eq!(m.additional_context.as_deref(), Some("ctx-a\nctx-b"));
        assert_eq!(m.reason.as_deref(), Some("bad"));
    }

    #[test]
    fn input_payload_shape() {
        let targets = vec!["vps-1".to_string()];
        let args = json!({"command":"ls"});
        let input = HookEventInput {
            event: HookEvent::PreToolUse,
            session_id: "s1",
            cwd: "/tmp",
            workspace_id: Some("ws1"),
            vps_targets: &targets,
            tool_name: Some("run_command"),
            tool_input: Some(&args),
            tool_response: None,
            prompt: None,
        };
        let v = input.to_json();
        assert_eq!(v["hook_event_name"], "PreToolUse");
        assert_eq!(v["tool_name"], "run_command");
        assert_eq!(v["tool_input"]["command"], "ls");
        assert_eq!(v["vps_targets"][0], "vps-1");
        assert_eq!(v["workspace_id"], "ws1");
        assert!(v.get("prompt").is_none());
    }

    #[tokio::test]
    async fn live_runner_blocks_on_exit_2() {
        // A real PreToolUse hook that exits 2 must block the tool. The shell wrapper is
        // added by the runner, so "exit 2" is portable across cmd and sh.
        let cfg = HooksConfig::parse(
            r#"{"PreToolUse":[{"matcher":"*","hooks":[{"command":"exit 2"}]}]}"#,
        )
        .unwrap();
        let targets: Vec<String> = vec![];
        let args = json!({});
        let input = HookEventInput {
            event: HookEvent::PreToolUse,
            session_id: "s",
            cwd: ".",
            workspace_id: None,
            vps_targets: &targets,
            tool_name: Some("run_command"),
            tool_input: Some(&args),
            tool_response: None,
            prompt: None,
        };
        let d = run_event(&cfg, &input).await;
        assert!(d.blocks(), "exit 2 hook should block");
    }

    #[tokio::test]
    async fn live_runner_allows_on_exit_0() {
        let cfg =
            HooksConfig::parse(r#"{"PreToolUse":[{"matcher":"*","hooks":[{"command":"exit 0"}]}]}"#)
                .unwrap();
        let targets: Vec<String> = vec![];
        let args = json!({});
        let input = HookEventInput {
            event: HookEvent::PreToolUse,
            session_id: "s",
            cwd: ".",
            workspace_id: None,
            vps_targets: &targets,
            tool_name: Some("run_command"),
            tool_input: Some(&args),
            tool_response: None,
            prompt: None,
        };
        let d = run_event(&cfg, &input).await;
        assert!(!d.blocks(), "exit 0 hook should not block");
    }
}
