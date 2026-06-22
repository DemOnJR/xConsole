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
        Self {
            api_key,
            base_url: base_url.filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_BASE.to_string()),
            http: reqwest::Client::new(),
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
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "stream": true,
            "messages": Self::build_messages(req),
        });
        let tools = Self::build_tools(req);
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("openai request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("openai error {status}: {text}"));
        }

        let mut out = ChatResponse::default();
        let mut sse = SseBuffer::new();
        let mut tools_acc: Vec<ToolAcc> = Vec::new();

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
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

        Ok(out)
    }
}
