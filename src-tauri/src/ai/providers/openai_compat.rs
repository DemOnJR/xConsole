//! OpenAI-compatible Chat Completions provider (streaming).
//!
//! Covers any endpoint speaking the OpenAI wire format: custom/self-hosted
//! gateways, OpenAI itself, and the Cursor API.

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::{join_url, SseBuffer};
use crate::ai::provider::{
    emit, ChatRequest, ChatResponse, EventSink, Provider, StreamEvent, StreamStats, ToolCall,
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
                "user" => out.push(json!({
                    "role": "user",
                    "content": crate::ai::vision::openai_user_content(&m.content, &m.images),
                })),
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
                        // Pass back reasoning_content when intermediate tool calls exist to maintain KV-cache prefix
                        if let Some(r) = &m.reasoning_content {
                            if !r.is_empty() {
                                msg["reasoning_content"] = json!(r);
                            }
                        }
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

#[derive(Debug, Default, PartialEq, Eq)]
struct UsageCounts {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    cached_tokens: Option<u32>,
    cache_write_tokens: Option<u32>,
}

fn visible_response_content(content: String, _opaque_reasoning: String) -> String {
    content
}

fn usage_counts(event: &Value) -> UsageCounts {
    let Some(usage) = event.get("usage") else {
        return UsageCounts::default();
    };
    let count = |value: Option<&Value>| value.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    let details = usage.get("prompt_tokens_details");
    UsageCounts {
        prompt_tokens: count(usage.get("prompt_tokens")),
        completion_tokens: count(usage.get("completion_tokens")),
        // OpenAI: prompt_tokens_details.cached_tokens
        // DeepSeek / Command Code: usage.prompt_cache_hit_tokens (and the same
        // value mirrored under details.cached_tokens).
        cached_tokens: count(details.and_then(|v| v.get("cached_tokens")))
            .or_else(|| count(details.and_then(|v| v.get("cache_read_input_tokens"))))
            .or_else(|| count(usage.get("prompt_cache_hit_tokens")))
            .or_else(|| count(usage.get("cached_tokens"))),
        cache_write_tokens: count(usage.get("cache_write_tokens"))
            .or_else(|| count(details.and_then(|v| v.get("cache_write_tokens")))),
    }
}

/// GPT-5.x / native OpenAI accept `prompt_cache_options`.
fn wants_openai_explicit_cache_options(model: &str, base_url: &str) -> bool {
    let m = model.to_lowercase();
    let url = base_url.to_lowercase();
    (m.contains("gpt-5") || m.contains("gpt-4.5") || url.contains("api.openai.com"))
        && !url.contains("openrouter")
        && !url.contains("deepseek")
}

fn wants_openai_implicit_cache_options(model: &str, base_url: &str) -> bool {
    let m = model.to_lowercase();
    let url = base_url.to_lowercase();
    !wants_openai_explicit_cache_options(model, base_url)
        && (m.contains("gpt-4") || url.contains("api.openai.com"))
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
                "stream_options": { "include_usage": true },
                "messages": Self::build_messages(req),
            });
            // Reasoning effort → OpenAI reasoning_effort (low/medium/high), off/empty = default.
            if !req.reasoning.is_empty() && req.reasoning != "off" {
                body["reasoning_effort"] = json!(req.reasoning);
            }
            // Stable cache key routes every request of a session to the same cache
            // node (OpenAI, OpenCode Go, most OpenAI-compat proxies). DeepSeek's
            // automatic prefix cache ignores the field; unknown-field 400s are
            // rare and cheaper than a full miss on a routed-away prefix.
            if !req.session_id.is_empty() {
                body["prompt_cache_key"] = json!(format!("xc-{}", req.session_id));
                if wants_openai_explicit_cache_options(&req.model, &self.base_url) {
                    body["prompt_cache_options"] = json!({ "mode": "explicit", "ttl": "30m" });
                } else if wants_openai_implicit_cache_options(&req.model, &self.base_url) {
                    body["prompt_cache_options"] = json!({ "mode": "implicit" });
                }
            }
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
        let started = std::time::Instant::now();
        let mut usage = UsageCounts::default();

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
                let counts = usage_counts(&ev);
                usage.prompt_tokens = counts.prompt_tokens.or(usage.prompt_tokens);
                usage.completion_tokens = counts.completion_tokens.or(usage.completion_tokens);
                usage.cached_tokens = counts.cached_tokens.or(usage.cached_tokens);
                usage.cache_write_tokens = counts.cache_write_tokens.or(usage.cache_write_tokens);

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

        // Provider reasoning is opaque continuation state, not user-visible content.
        // Never promote it into ChatResponse.content, persistence, compaction, or export.
        out.content = visible_response_content(out.content, reasoning.clone());
        out.reasoning_content = if reasoning.is_empty() { None } else { Some(reasoning) };

        out.prompt_tokens = usage.prompt_tokens;
        out.cached_tokens = usage.cached_tokens;
        out.completion_tokens = usage.completion_tokens;

        if let Some(completion_tokens) = usage.completion_tokens {
            let duration_ms = started.elapsed().as_millis() as u64;
            let seconds = (duration_ms as f64 / 1000.0).max(0.05);
            emit(
                sink,
                StreamEvent::Stats(StreamStats {
                    completion_tokens,
                    prompt_tokens: usage.prompt_tokens,
                    cached_tokens: usage.cached_tokens,
                    cache_creation_tokens: usage.cache_write_tokens,
                    duration_ms: duration_ms.max(1),
                    tokens_per_sec: (completion_tokens as f64 / seconds) as f32,
                }),
            );
            emit(
                sink,
                StreamEvent::Cost(crate::ai::cost::turn_cost(
                    &crate::ai::cost::kind_for_model("openai", &req.model),
                    &req.model,
                    usage.prompt_tokens,
                    completion_tokens,
                    usage.cached_tokens,
                    usage.cache_write_tokens,
                )),
            );
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_reasoning_is_never_promoted_to_visible_content() {
        assert_eq!(visible_response_content(String::new(), "private reasoning".into()), "");
        assert_eq!(visible_response_content("visible answer".into(), "private reasoning".into()), "visible answer");
    }

    #[test]
    fn extracts_standard_and_compatibility_cache_fields() {
        let standard: Value = serde_json::json!({
            "usage": {
                "prompt_tokens": 42,
                "completion_tokens": 7,
                "prompt_tokens_details": { "cached_tokens": 19 }
            }
        });
        assert_eq!(
            usage_counts(&standard),
            UsageCounts {
                prompt_tokens: Some(42),
                completion_tokens: Some(7),
                cached_tokens: Some(19),
                cache_write_tokens: None,
            }
        );

        let alias: Value = serde_json::json!({
            "usage": {
                "prompt_tokens": 42,
                "completion_tokens": 7,
                "prompt_tokens_details": { "cache_read_input_tokens": 19 }
            }
        });
        assert_eq!(usage_counts(&alias).cached_tokens, Some(19));

        let top_level: Value = serde_json::json!({
            "usage": { "prompt_tokens": 42, "completion_tokens": 7, "cached_tokens": 19 }
        });
        assert_eq!(usage_counts(&top_level).cached_tokens, Some(19));

        // DeepSeek V4 / Command Code official field.
        let deepseek: Value = serde_json::json!({
            "usage": {
                "prompt_tokens": 1387,
                "completion_tokens": 423,
                "prompt_tokens_details": { "cached_tokens": 128, "miss_tokens": 1259 },
                "prompt_cache_hit_tokens": 128,
                "prompt_cache_miss_tokens": 1259
            }
        });
        assert_eq!(usage_counts(&deepseek).cached_tokens, Some(128));
        assert_eq!(usage_counts(&deepseek).prompt_tokens, Some(1387));

        let hit_only: Value = serde_json::json!({
            "usage": {
                "prompt_tokens": 2000,
                "completion_tokens": 10,
                "prompt_cache_hit_tokens": 1856
            }
        });
        assert_eq!(usage_counts(&hit_only).cached_tokens, Some(1856));
    }

    #[test]
    fn openai_cache_options_only_for_native_openai() {
        assert!(wants_openai_explicit_cache_options("gpt-5.6-luna", "https://api.openai.com/v1"));
        assert!(wants_openai_explicit_cache_options("gpt-5", "https://example.com/v1"));
        assert!(!wants_openai_explicit_cache_options(
            "deepseek/deepseek-v4-flash",
            "https://api.commandcode.ai/provider/v1"
        ));
        assert!(!wants_openai_explicit_cache_options(
            "deepseek-v4-flash",
            "https://api.deepseek.com/v1"
        ));
    }

    #[test]
    fn usage_only_event_is_parsed_without_choices() {
        let event: Value = serde_json::json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 5,
                "prompt_tokens_details": { "cached_tokens": 80 }
            }
        });
        let counts = usage_counts(&event);
        assert_eq!(counts.prompt_tokens, Some(100));
        assert_eq!(counts.completion_tokens, Some(5));
        assert_eq!(counts.cached_tokens, Some(80));
    }

    #[test]
    fn malformed_or_missing_cache_is_optional() {
        let event: Value = serde_json::json!({
            "usage": { "prompt_tokens": 4, "completion_tokens": 1, "cached_tokens": "unknown" }
        });
        assert_eq!(usage_counts(&event).cached_tokens, None);
        assert_eq!(usage_counts(&serde_json::json!({})), UsageCounts::default());
    }

    #[test]
    fn sse_buffer_handles_split_usage_event_and_done() {
        let mut sse = SseBuffer::new();
        assert!(sse.push("data: {\"choices\":[],\"usage\":{\"prompt_tokens\":").is_empty());
        let payloads = sse.push("4,\"completion_tokens\":1}}\r\n\r\ndata: [DONE]\r\n\r\n");
        assert_eq!(payloads.len(), 2);
        let event: Value = serde_json::from_str(&payloads[0]).unwrap();
        assert_eq!(usage_counts(&event).completion_tokens, Some(1));
        assert_eq!(payloads[1], "[DONE]");
    }
}
