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
    ///
    /// Do **not** put `cache_control` on the last user message. That breakpoint
    /// moves every turn (and our dynamic runtime block lives inside it), which
    /// drops `cache_control` from the previous last-user and busts the history
    /// prefix. System + last tool stay stable across turns; history then caches
    /// as an implicit prefix behind those breakpoints.
    fn build_messages(messages: &[ChatMessage]) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for m in messages {
            match m.role.as_str() {
                "user" => {
                    out.push(json!({
                        "role": "user",
                        "content": crate::ai::vision::anthropic_user_content(&m.content, &m.images),
                    }));
                }
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
                    // The Messages API rejects an assistant turn with empty
                    // content; skip it so the next request isn't 400'd.
                    if blocks.is_empty() {
                        continue;
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
        let count = req.tools.len();
        req.tools
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut def = json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                });
                // Cache the tools block: mark the LAST tool def as the breakpoint so
                // the whole tools+system prefix is one cached unit (Anthropic caches
                // everything up to the last breakpoint).
                if i == count - 1 {
                    def["cache_control"] = json!({"type": "ephemeral"});
                }
                def
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
        // Long retention = 1h cache TTL (2× write price); short/empty = 5 min default.
        let long_cache = req.cache_retention == "long";
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "stream": true,
            "messages": Self::build_messages(&req.messages),
        });
        // Reasoning effort → Anthropic extended thinking budget (only when enabled).
        // off/empty leaves the provider default; low/medium/high map to token budgets.
        if !req.reasoning.is_empty() && req.reasoning != "off" {
            let budget = match req.reasoning.as_str() {
                "low" => 2048,
                "high" => 16384,
                _ => 8192, // medium / anything else
            };
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
            // Thinking requires temperature 1 (Anthropic constraint).
            body["temperature"] = json!(1.0);
        }
        // Prompt caching: mark the static system prefix as ephemeral so multi-turn
        // agent loops reuse Anthropic's server-side cache (up to ~90% latency cut).
        if !req.system.is_empty() {
            let mut block = json!({
                "type": "text",
                "text": req.system,
                "cache_control": { "type": "ephemeral" }
            });
            if long_cache {
                block["cache_control"]["ttl"] = json!("1h");
            }
            body["system"] = json!([block]);
        }
        let tools = Self::build_tools(req);
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        let mut beta = "prompt-caching-2024-07-31".to_string();
        if long_cache {
            // Extended TTL requires its own beta header; without it the 1h TTL
            // silently degrades to 5 minutes and re-bills every turn.
            beta.push_str(",extended-cache-ttl-2025-01-23");
        }

        let resp = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            // Enable prompt-caching beta for cache_control on system blocks.
            .header("anthropic-beta", beta)
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
        let mut input_tokens: Option<u32> = None;
        let mut output_tokens: Option<u32> = None;
        let mut cache_read_tokens: Option<u32> = None;
        let mut cache_write_tokens: Option<u32> = None;
        let started = std::time::Instant::now();

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            // User pressed Stop — abort the in-flight response immediately.
            if req.is_cancelled() {
                emit(sink, StreamEvent::Status("Stopped.".into()));
                break;
            }
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
                    "message_start" => {
                        if let Some(u) = ev.get("message").and_then(|m| m.get("usage")) {
                            input_tokens = u
                                .get("input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32);
                            cache_read_tokens = u
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32)
                                .or(cache_read_tokens);
                            cache_write_tokens = u
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32)
                                .or(cache_write_tokens);
                        }
                    }
                    "content_block_start" => {
                        let block = &ev["content_block"];
                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            tool_acc.push((
                                block["id"].as_str().unwrap_or("").to_string(),
                                block["name"].as_str().unwrap_or("").to_string(),
                                String::new(),
                            ));
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
                    "message_delta" => {
                        if let Some(sr) = ev["delta"].get("stop_reason").and_then(|v| v.as_str()) {
                            out.stop_reason = sr.to_string();
                        }
                        if let Some(u) = ev.get("usage") {
                            output_tokens = u
                                .get("output_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32)
                                .or(output_tokens);
                            cache_read_tokens = u
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32)
                                .or(cache_read_tokens);
                        }
                    }
                    _ => {}
                }
            }
        }

        for (id, name, args_str) in tool_acc {
            let arguments: Value = serde_json::from_str(&args_str).unwrap_or(json!({}));
            let tc = ToolCall { id, name, arguments };
            emit(sink, StreamEvent::ToolCall(tc.clone()));
            out.tool_calls.push(tc);
        }

        out.prompt_tokens = input_tokens;
        out.cached_tokens = cache_read_tokens;

        if let Some(completion) = output_tokens {
            let ms = started.elapsed().as_millis() as u64;
            let secs = (ms as f64 / 1000.0).max(0.05);
            emit(
                sink,
                StreamEvent::Stats(crate::ai::provider::StreamStats {
                    completion_tokens: completion,
                    prompt_tokens: input_tokens,
                    cached_tokens: cache_read_tokens,
                    cache_creation_tokens: cache_write_tokens,
                    duration_ms: ms.max(1),
                    tokens_per_sec: (completion as f64 / secs) as f32,
                }),
            );
            emit(
                sink,
                StreamEvent::Cost(crate::ai::cost::turn_cost(
                    "anthropic",
                    &req.model,
                    input_tokens,
                    completion,
                    cache_read_tokens,
                    cache_write_tokens,
                )),
            );
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::{ChatMessage, ChatRequest, ToolDef};

    #[test]
    fn last_tool_def_gets_cache_control() {
        let mut req = ChatRequest::new("claude-sonnet-4");
        req.tools = vec![
            ToolDef {
                name: "a".into(),
                description: "a".into(),
                parameters: json!({}),
            },
            ToolDef {
                name: "b".into(),
                description: "b".into(),
                parameters: json!({}),
            },
        ];
        let tools = AnthropicProvider::build_tools(&req);
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn messages_never_carry_moving_cache_breakpoints() {
        let msgs = vec![
            ChatMessage::user("one"),
            ChatMessage::assistant("ok"),
            ChatMessage::tool_result("call-1", "output"),
            ChatMessage::user("two"),
        ];
        let built = AnthropicProvider::build_messages(&msgs);
        for m in &built {
            if m["role"] == "user" && m["content"].is_string() {
                // Plain user text — no cache_control (would move next turn).
                continue;
            }
            if m["content"].is_array() {
                for block in m["content"].as_array().unwrap() {
                    assert!(
                        block.get("cache_control").is_none(),
                        "history blocks must not carry cache_control: {block}"
                    );
                }
            }
        }
    }
}
