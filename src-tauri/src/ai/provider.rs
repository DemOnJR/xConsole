//! Provider-agnostic chat types and the `Provider` trait.
//!
//! Every backend (Anthropic, OpenAI-compatible, Cursor, Codex/OpenCode CLI)
//! implements one trait so the agent loop never branches on provider type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

/// A tool the model may call. `parameters` is a JSON Schema object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A model-issued tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// An image attached to a user turn (base64, latest message only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatImage {
    pub media_type: String,
    pub data: String,
    #[serde(default)]
    pub name: String,
}

/// One message in a conversation. `role` is "system" | "user" | "assistant" | "tool".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
    /// Tool calls issued by the assistant in this message.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// For role == "tool": the id of the tool call this result answers.
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// Pixels for this turn. Only the latest user message is sent to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImage>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
            images: vec![],
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
            images: vec![],
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: Some(tool_call_id.into()),
            images: vec![],
        }
    }
}

/// OpenAI / DeepSeek reject a request if an assistant `tool_calls` message is
/// not followed by a `tool` result for every id. That happens when a previous
/// turn was stopped mid-loop (or used to hit a 20-iter cap). Insert stubs so
/// the next user message can proceed.
pub fn close_unanswered_tool_calls(messages: &mut Vec<ChatMessage>) -> usize {
    let mut added = 0;
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" || messages[i].tool_calls.is_empty() {
            i += 1;
            continue;
        }
        let calls = messages[i].tool_calls.clone();
        let mut have = std::collections::HashSet::new();
        let mut j = i + 1;
        while j < messages.len() && messages[j].role == "tool" {
            if let Some(id) = &messages[j].tool_call_id {
                have.insert(id.clone());
            }
            j += 1;
        }
        let mut insert_at = j;
        for call in calls {
            if have.contains(&call.id) {
                continue;
            }
            messages.insert(
                insert_at,
                ChatMessage::tool_result(
                    call.id,
                    "error: tool call was interrupted before a result was recorded",
                ),
            );
            insert_at += 1;
            added += 1;
        }
        i = insert_at;
    }
    added
}

/// VPS execution context passed to Cursor CLI (MCP bridge).
#[derive(Debug, Clone)]
pub struct XConsoleExec {
    pub data_dir: PathBuf,
    pub session_id: String,
    pub targets: Vec<String>,
    pub safety: String,
    /// Active workspace id (empty if none) — lets the MCP write the project brief
    /// and workspace-scoped memory for the right workspace.
    pub workspace_id: String,
}

/// A single chat request to a provider.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// When set, Cursor CLI uses xConsole MCP for SSH on selected VPS targets.
    pub xconsole: Option<XConsoleExec>,
    /// User-pressed-Stop flag. Providers poll this in their streaming loop to abort
    /// an in-flight response immediately. `None` means no cancellation wired.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Cache retention: "short" (5 min) or "long" (1h, 2× write price). Passed to
    /// providers that support explicit cache TTLs; empty = provider default.
    pub cache_retention: String,
    /// Stable session id for provider cache routing (OpenAI prompt_cache_key).
    pub session_id: String,
    /// Reasoning effort: "off" | "low" | "medium" | "high". Empty = provider default.
    pub reasoning: String,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: String::new(),
            messages: vec![],
            tools: vec![],
            max_tokens: 16_384,
            temperature: 0.7,
            xconsole: None,
            cancel: None,
            cache_retention: String::new(),
            session_id: String::new(),
            reasoning: String::new(),
        }
    }

    /// True when the user has pressed Stop mid-stream.
    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .map(|c| c.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap_or(false)
    }
}

/// The full result of one chat turn.
#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: String,
    /// Prompt tokens for this HTTP request (when the provider reported usage).
    pub prompt_tokens: Option<u32>,
    /// Cached prompt tokens for this HTTP request.
    pub cached_tokens: Option<u32>,
    /// Completion tokens when the provider reported usage (used to detect a cap hit).
    pub completion_tokens: Option<u32>,
}

/// True when the model stopped because it hit the output-token cap, not because
/// it finished. A truncated reply often has no tool_calls even though work is
/// unfinished — the agent loop must continue, not treat it as "done".
pub fn is_output_truncated(stop_reason: &str, completion_tokens: Option<u32>, max_tokens: u32) -> bool {
    let r = stop_reason.trim().to_ascii_lowercase();
    if matches!(
        r.as_str(),
        "length" | "max_tokens" | "max_output_tokens" | "max_output" | "token_limit"
    ) {
        return true;
    }
    if r.contains("max_token") || r.contains("token limit") {
        return true;
    }
    match completion_tokens {
        Some(n) if max_tokens > 0 && n >= max_tokens.saturating_sub(1) => true,
        // Many OpenAI-compat hosts ignore our max_tokens and silently cap at 4K/8K
        // while still sending finish_reason=stop. The UI showed exactly 4096 tok.
        Some(n) if n == 4096 || n == 8192 => true,
        _ => false,
    }
}

/// Prose that still has open checklist rows — the model talked instead of calling tools.
pub fn reply_has_open_checklist(content: &str) -> bool {
    content.lines().any(|line| {
        let t = line.trim();
        t.starts_with("[ ]")
            || t.starts_with("[>]")
            || t.contains("[pending]")
            || t.contains("[in_progress]")
    })
}

#[cfg(test)]
mod truncated_tests {
    use super::{is_output_truncated, reply_has_open_checklist};

    #[test]
    fn detects_length_and_cap() {
        assert!(is_output_truncated("length", None, 4096));
        assert!(is_output_truncated("max_tokens", None, 4096));
        assert!(is_output_truncated("stop", Some(4096), 4096));
        assert!(is_output_truncated("stop", Some(4096), 16_384));
        assert!(!is_output_truncated("stop", Some(200), 4096));
        assert!(!is_output_truncated("", None, 4096));
    }

    #[test]
    fn open_checklist_in_prose() {
        assert!(reply_has_open_checklist("[>] Inspect ufw\n[ ] Write jail"));
        assert!(!reply_has_open_checklist("[x] Inspect ufw\n[x] Write jail"));
    }
}

/// One line in a compact file diff (Cursor-style).
#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    pub kind: String,
    pub text: String,
}

/// Structured activity for the live agent timeline in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum ActivityEvent {
    /// A tool invocation started.
    ToolStart {
        id: String,
        tool: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A tool invocation finished.
    ToolEnd {
        id: String,
        ok: bool,
    },
    /// Agent read a skill playbook.
    SkillRead {
        id: String,
        category: String,
        name: String,
    },
    /// Agent saved a new/updated skill.
    SkillSaved {
        id: String,
        category: String,
        name: String,
    },
    /// SSH command about to run (or running).
    Command {
        id: String,
        vps: String,
        command: String,
    },
    /// Local or remote file edit with line diff stats.
    FileEdit {
        id: String,
        path: String,
        lines_added: usize,
        lines_removed: usize,
        hunks: Vec<DiffLine>,
    },
}

/// Token throughput reported when a provider exposes usage (e.g. Ollama eval_count).
#[derive(Debug, Clone, Serialize)]
pub struct StreamStats {
    pub completion_tokens: u32,
    pub prompt_tokens: Option<u32>,
    /// Tokens served from provider prompt cache (Anthropic / OpenAI when reported).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    /// Tokens written to the provider cache this request (cache misses).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u32>,
    pub duration_ms: u64,
    pub tokens_per_sec: f32,
}

/// Estimated context window fill before the model call (~4 chars/token).
#[derive(Debug, Clone, Serialize)]
pub struct TurnTelemetryEvent {
    pub tool_calls: u64,
    pub tool_cache_lookups: u64,
    pub tool_cache_hits: u64,
    pub tool_cache_misses: u64,
    pub tool_cache_writes: u64,
    pub tool_cache_hit_rate: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrefixTelemetryEvent {
    pub request_index: u32,
    pub system_hash: String,
    pub schema_hash: String,
    pub message_prefix_hash: String,
    pub system_bytes: u64,
    pub schema_bytes: u64,
    pub message_bytes: u64,
    pub classification: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageEvent {
    pub segments: Vec<ContextUsageSegment>,
    pub total_tokens: u32,
    pub context_limit: u32,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageSegment {
    pub key: String,
    pub label: String,
    pub tokens: u32,
}

/// Streaming events emitted to the UI during a turn.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum StreamEvent {
    /// A chunk of assistant text.
    Text(String),
    /// Final token throughput for this generation leg.
    Stats(StreamStats),
    /// Estimated per-turn cost + cache economics (provider usage → USD).
    Cost(crate::ai::cost::TurnCost),
    /// Per-turn tool and cache counters.
    TurnTelemetry(TurnTelemetryEvent),
    /// Privacy-safe provider-prefix fingerprints and stability classification.
    PrefixTelemetry(PrefixTelemetryEvent),
    /// Estimated prompt context breakdown for this turn.
    ContextUsage(ContextUsageEvent),
    /// Conversation history replaced after auto-compaction (Hermes-style).
    ConversationCompacted { messages: Vec<ChatMessage> },
    /// A status note (e.g. "running command...").
    Status(String),
    /// A tool call the agent is about to execute.
    ToolCall(ToolCall),
    /// Output captured from a tool execution.
    ToolResult { id: String, output: String },
    /// Live activity step (skills, commands, tools).
    Activity(ActivityEvent),
    /// The turn finished.
    Done,
    /// A fatal error for this turn.
    Error(String),
}

pub type EventSink = UnboundedSender<StreamEvent>;

/// Emit an event if a sink is attached (best-effort; ignores closed channels).
pub fn emit(sink: Option<&EventSink>, ev: StreamEvent) {
    if let Some(tx) = sink {
        let _ = tx.send(ev);
    }
}

/// A chat backend. One trait, many implementations.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Run one chat turn. When `sink` is set, stream text deltas through it.
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String>;

    /// Whether this provider runs an external agent that does its own tool use
    /// (CLI providers). The agent loop skips our tool loop for these.
    fn is_autonomous_cli(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod close_tool_tests {
    use super::*;

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "run_command".into(),
            arguments: serde_json::json!({"command": "true"}),
        }
    }

    #[test]
    fn inserts_missing_results_before_the_next_user_message() {
        let mut msgs = vec![
            ChatMessage::user("harden ssh"),
            {
                let mut a = ChatMessage::assistant("checking");
                a.tool_calls = vec![call("a"), call("b")];
                a
            },
            ChatMessage::user("re check the vps"),
        ];
        assert_eq!(close_unanswered_tool_calls(&mut msgs), 2);
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].tool_call_id.as_deref(), Some("a"));
        assert_eq!(msgs[3].role, "tool");
        assert_eq!(msgs[3].tool_call_id.as_deref(), Some("b"));
        assert_eq!(msgs[4].role, "user");
        assert_eq!(close_unanswered_tool_calls(&mut msgs), 0);
    }

    #[test]
    fn keeps_existing_results_and_fills_only_the_gap() {
        let mut msgs = vec![
            {
                let mut a = ChatMessage::assistant("go");
                a.tool_calls = vec![call("a"), call("b")];
                a
            },
            ChatMessage::tool_result("a", "ok"),
        ];
        assert_eq!(close_unanswered_tool_calls(&mut msgs), 1);
        assert_eq!(msgs[2].tool_call_id.as_deref(), Some("b"));
    }
}
