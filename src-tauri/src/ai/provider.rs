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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// One message in a conversation. `role` is "system" | "user" | "assistant" | "tool".
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into(), tool_calls: vec![], tool_call_id: None }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into(), tool_calls: vec![], tool_call_id: None }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: Some(tool_call_id.into()),
        }
    }
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
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: String::new(),
            messages: vec![],
            tools: vec![],
            max_tokens: 4096,
            temperature: 0.7,
            xconsole: None,
            cancel: None,
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
    pub duration_ms: u64,
    pub tokens_per_sec: f32,
}

/// Estimated context window fill before the model call (~4 chars/token).
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
