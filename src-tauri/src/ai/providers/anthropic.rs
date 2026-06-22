//! Anthropic Messages API provider (streaming).

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::{join_url, SseBuffer};
use crate::ai::provider::{
    emit, ChatMessage, ChatRequest, ChatResponse, EventSink, Provider, StreamEvent, ToolCall,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_BASE: &str = "https://api.anthropic.com";

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_BASE.to_string()),
            http: reqwest::Client::new(),
        }
    }

    /// Convert our portable messages into Anthropic's content-block format.
    fn build_messages(messages: &[ChatMessage]) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for m in messages {
            match m.role.as_str() {
                "user" => out.push(json!({"role": "user", "content": m.content})),
                "assistant" => {
                    let mut blocks: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        blocks.push(json!({"type": "text", "text": m.content}));
                    }
                    for tc in &m.tool_calls {
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments,
                        }));
                    }
                    out.push(json!({"role": "assistant", "content": blocks}));
                }
                "tool" => {
                    // Tool results are user-role messages in Anthropic's schema.
                    out.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                            "content": m.content,
                        }],
                    }));
                }
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
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String> {
        let url = join_url(&self.base_url, "v1/messages");
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "stream": true,
            "messages": Self::build_messages(&req.messages),
        });
        if !req.system.is_empty() {
            body["system"] = json!(req.system);
        }
        let tools = Self::build_tools(req);
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        let resp = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("anthropic request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("anthropic error {status}: {text}"));
        }

        let mut out = ChatResponse::default();
        let mut sse = SseBuffer::new();
        // Tool-call accumulation: index -> (id, name, json string)
        let mut tool_acc: Vec<(String, String, String)> = Vec::new();
        let mut cur_block_is_tool = false;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("anthropic stream error: {e}"))?;
            let text = String::from_utf8_lossy(&chunk);
            for payload in sse.push(&text) {
                if payload == "[DONE]" {
                    continue;
                }
                let ev: Value = match serde_json::from_str(&payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match ev.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "content_block_start" => {
                        let block = &ev["content_block"];
                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            cur_block_is_tool = true;
                            tool_acc.push((
                                block["id"].as_str().unwrap_or("").to_string(),
                                block["name"].as_str().unwrap_or("").to_string(),
                                String::new(),
                            ));
                        } else {
                            cur_block_is_tool = false;
                        }
                    }
                    "content_block_delta" => {
                        let delta = &ev["delta"];
                        match delta.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                            "text_delta" => {
                                if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                                    out.content.push_str(t);
                                    emit(sink, StreamEvent::Text(t.to_string()));
                                }
                            }
                            "input_json_delta" => {
                                if let (Some(last), Some(pj)) =
                                    (tool_acc.last_mut(), delta.get("partial_json").and_then(|v| v.as_str()))
                                {
                                    last.2.push_str(pj);
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        cur_block_is_tool = false;
                    }
                    "message_delta" => {
                        if let Some(sr) = ev["delta"].get("stop_reason").and_then(|v| v.as_str()) {
                            out.stop_reason = sr.to_string();
                        }
                    }
                    _ => {}
                }
                let _ = cur_block_is_tool;
            }
        }

        for (id, name, args_str) in tool_acc {
            let arguments: Value = serde_json::from_str(&args_str).unwrap_or(json!({}));
            let tc = ToolCall { id, name, arguments };
            emit(sink, StreamEvent::ToolCall(tc.clone()));
            out.tool_calls.push(tc);
        }

        Ok(out)
    }
}
