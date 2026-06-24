//! OpenAI-compatible Chat Completions provider (streaming).
//!
//! Covers any endpoint speaking the OpenAI wire format: custom/self-hosted
//! gateways, OpenAI itself, and the Cursor API.

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::{join_url, SseBuffer};
use crate::ai::provider::{
    emit, ChatRequest, ChatResponse, EventSink, Provider, StreamEvent, ToolCall,
};

const DEFAULT_BASE: &str = "https://api.openai.com/v1";

pub struct OpenAiProvider {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        // A bounded connect timeout + short idle-pool so we don't reuse a stale
        // keep-alive connection a cloud host (e.g. Groq) has already closed —
        // which surfaces as a spurious "could not reach the server" error.
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .pool_idle_timeout(std::time::Duration::from_secs(15))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            api_key,
            base_url: base_url.filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_BASE.to_string()),
            http,
        }
    }

    fn build_messages(req: &ChatRequest) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        if !req.system.is_empty() {
            out.push(json!({"role": "system", "content": req.system}));
        }
        for m in &req.messages {
            match m.role.as_str() {
                "user" => out.push(json!({"role": "user", "content": m.content})),
                "assistant" => {
                    let mut msg = json!({"role": "assistant", "content": m.content});
                    if !m.tool_calls.is_empty() {
                        msg["tool_calls"] = json!(m
                            .tool_calls
                            .iter()
                            .map(|tc| json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                },
                            }))
                            .collect::<Vec<_>>());
                    }
                    out.push(msg);
                }
                "tool" => out.push(json!({
                    "role": "tool",
                    "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content,
                })),
                _ => {}
            }
        }
        out
    }

    fn build_tools(req: &ChatRequest) -> Vec<Value> {
        req.tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    },
                })
            })
            .collect()
    }
}

/// Accumulator for one streamed tool call (arguments arrive as string fragments).
#[derive(Default)]
struct ToolAcc {
    id: String,
    name: String,
    args: String,
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String> {
        let url = join_url(&self.base_url, "chat/completions");
        let tools = Self::build_tools(req);

        // Send the request. If the model rejects tool calling (e.g. some hosted
        // Groq models), retry once WITHOUT tools so plain chat still works — and
        // tell the user, since the agent can't run commands without tools.
        let mut send_tools = !tools.is_empty();
        let resp = loop {
            let mut body = json!({
                "model": req.model,
                "max_tokens": req.max_tokens,
                "temperature": req.temperature,
                "stream": true,
                "messages": Self::build_messages(req),
            });
            if send_tools {
                body["tools"] = json!(tools);
            }

            // Send with a small retry on transient connection failures — a stale
            // pooled keep-alive connection to a cloud host closes intermittently.
            let is_local = url.contains("127.0.0.1") || url.contains("localhost");
            let mut attempt = 0u8;
            let resp = loop {
                let mut builder = self
                    .http
                    .post(&url)
                    .header("content-type", "application/json");
                // Self-hosted llama.cpp servers need no key; only send auth when present.
                if !self.api_key.is_empty() {
                    builder = builder.bearer_auth(&self.api_key);
                }
                match builder.json(&body).send().await {
                    Ok(r) => break r,
                    Err(e)
                        if (e.is_connect() || e.is_timeout() || e.is_request()) && attempt < 2 =>
                    {
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(
                            300 * attempt as u64,
                        ))
                        .await;
                    }
                    Err(e) => {
                        return Err(if (e.is_connect() || e.is_timeout()) && is_local {
                            format!(
                                "could not reach the local model server at {url} — is it running? \
                                 (llama.cpp: `llama-server -m <model.gguf> --port 8080`)"
                            )
                        } else if e.is_connect() || e.is_timeout() {
                            format!(
                                "could not reach {url} — check your internet connection or the \
                                 provider's status, and that the Base URL is correct."
                            )
                        } else {
                            format!("request failed: {e}")
                        });
                    }
                }
            };

            if resp.status().is_success() {
                break resp;
            }
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if send_tools
                && status.as_u16() == 400
                && text.to_lowercase().contains("tool")
            {
                emit(
                    sink,
                    StreamEvent::Status(
                        "This model doesn't support tool calling — replying without tools. \
                         For SSH/VPS actions pick a tool-capable model (e.g. Groq \
                         `llama-3.3-70b-versatile`, or OpenAI/Anthropic/Cursor)."
                            .into(),
                    ),
                );
                send_tools = false;
                continue;
            }
            return Err(format!("openai error {status}: {text}"));
        };

        let mut out = ChatResponse::default();
        let mut sse = SseBuffer::new();
        let mut tools_acc: Vec<ToolAcc> = Vec::new();
        // Reasoning models (gpt-oss, qwen3, … on Groq) stream their text in a
        // separate `reasoning` field and may leave `content` empty.
        let mut reasoning = String::new();

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            // User pressed Stop — abort the in-flight response immediately.
            if req.is_cancelled() {
                emit(sink, StreamEvent::Status("Stopped.".into()));
                break;
            }
            let chunk = chunk.map_err(|e| format!("openai stream error: {e}"))?;
            let text = String::from_utf8_lossy(&chunk);
            for payload in sse.push(&text) {
                if payload == "[DONE]" {
                    continue;
                }
                let ev: Value = match serde_json::from_str(&payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let choice = match ev["choices"].get(0) {
                    Some(c) => c,
                    None => continue,
                };
                let delta = &choice["delta"];
                if let Some(t) = delta.get("content").and_then(|v| v.as_str()) {
                    if !t.is_empty() {
                        out.content.push_str(t);
                        emit(sink, StreamEvent::Text(t.to_string()));
                    }
                }
                for key in ["reasoning", "reasoning_content"] {
                    if let Some(t) = delta.get(key).and_then(|v| v.as_str()) {
                        if !t.is_empty() {
                            reasoning.push_str(t);
                        }
                    }
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        while tools_acc.len() <= idx {
                            tools_acc.push(ToolAcc::default());
                        }
                        let acc = &mut tools_acc[idx];
                        if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                            if !id.is_empty() {
                                acc.id = id.to_string();
                            }
                        }
                        if let Some(func) = tc.get("function") {
                            if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                                if !name.is_empty() {
                                    acc.name = name.to_string();
                                }
                            }
                            if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                                acc.args.push_str(args);
                            }
                        }
                    }
                }
                if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                    out.stop_reason = fr.to_string();
                }
            }
        }

        for acc in tools_acc {
            if acc.name.is_empty() {
                continue;
            }
            let arguments: Value = serde_json::from_str(&acc.args).unwrap_or(json!({}));
            let tc = ToolCall { id: acc.id, name: acc.name, arguments };
            emit(sink, StreamEvent::ToolCall(tc.clone()));
            out.tool_calls.push(tc);
        }

        // Reasoning model emitted only `reasoning` (no content, no tools) — surface
        // it so the reply isn't blank.
        if out.content.trim().is_empty() && out.tool_calls.is_empty() && !reasoning.trim().is_empty() {
            emit(sink, StreamEvent::Text(reasoning.clone()));
            out.content = reasoning;
        }

        Ok(out)
    }
}
