//! Local Ollama chat API (`/api/chat`) with streaming, tools, and qwen-style options.

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use super::join_url;
use crate::ai::provider::{
    emit, ChatMessage, ChatRequest, ChatResponse, EventSink, Provider, StreamEvent, StreamStats,
    ToolCall,
};

/// Default context for local models — 64K fits full VPS agent prompts on most hardware.
pub const DEFAULT_NUM_CTX: u32 = 65_536;

const DEFAULT_BASE: &str = "http://localhost:11434";

#[derive(Debug, Clone)]
pub struct OllamaOptions {
    pub num_ctx: u32,
    pub num_predict: Option<u32>,
    pub think: bool,
    pub keep_alive: String,
}

impl Default for OllamaOptions {
    fn default() -> Self {
        Self {
            num_ctx: DEFAULT_NUM_CTX,
            num_predict: None,
            think: false,
            keep_alive: "30m".into(),
        }
    }
}

pub fn parse_ollama_extra(extra_json: Option<&str>) -> OllamaOptions {
    let mut opts = OllamaOptions::default();
    let Some(raw) = extra_json.filter(|s| !s.trim().is_empty()) else {
        return opts;
    };
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return opts;
    };
    if let Some(n) = v.get("num_ctx").and_then(|x| x.as_u64()) {
        opts.num_ctx = n as u32;
    }
    opts.num_predict = v.get("num_predict").and_then(|x| {
        if x.is_null() {
            None
        } else {
            x.as_u64().map(|n| n as u32)
        }
    });
    if let Some(t) = v.get("think").and_then(|x| x.as_bool()) {
        opts.think = t;
    }
    if let Some(k) = v.get("keep_alive").and_then(|x| x.as_str()) {
        if !k.is_empty() {
            opts.keep_alive = k.to_string();
        }
    }
    opts
}

pub struct OllamaProvider {
    base_url: String,
    options: OllamaOptions,
    http: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>, options: OllamaOptions) -> Self {
        Self {
            base_url: base_url
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_BASE.to_string()),
            options,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    fn build_messages(req: &ChatRequest) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        if !req.system.is_empty() {
            out.push(json!({"role": "system", "content": req.system}));
        }
        let history = &req.messages;
        for (idx, m) in history.iter().enumerate() {
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
                                    "arguments": tc.arguments,
                                },
                            }))
                            .collect::<Vec<_>>());
                    }
                    out.push(msg);
                }
                "tool" => {
                    let mut msg = json!({"role": "tool", "content": m.content});
                    if let Some(name) = Self::resolve_tool_name(history, idx, m) {
                        msg["tool_name"] = json!(name);
                    }
                    out.push(msg);
                }
                _ => {}
            }
        }
        out
    }

    fn resolve_tool_name(
        history: &[ChatMessage],
        tool_idx: usize,
        tool_msg: &ChatMessage,
    ) -> Option<String> {
        let tc_id = tool_msg.tool_call_id.as_deref()?;
        for m in history[..tool_idx].iter().rev() {
            if m.role != "assistant" {
                continue;
            }
            for tc in &m.tool_calls {
                if tc.id == tc_id {
                    return Some(tc.name.clone());
                }
            }
        }
        None
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

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut options = json!({ "num_ctx": self.options.num_ctx });
        if let Some(n) = self.options.num_predict {
            options["num_predict"] = json!(n);
        }

        let mut body = json!({
            "model": req.model,
            "messages": Self::build_messages(req),
            "stream": stream,
            "think": self.options.think,
            "keep_alive": self.options.keep_alive,
            "options": options,
        });

        let tools = Self::build_tools(req);
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        body
    }

    fn parse_tool_calls(message: &OllamaMessage) -> Vec<ToolCall> {
        message
            .tool_calls
            .iter()
            .enumerate()
            .filter_map(|(i, tc)| {
                if tc.function.name.is_empty() {
                    return None;
                }
                let arguments = if tc.function.arguments.is_object() || tc.function.arguments.is_array()
                {
                    tc.function.arguments.clone()
                } else if let Some(s) = tc.function.arguments.as_str() {
                    serde_json::from_str(s).unwrap_or(json!({}))
                } else {
                    json!({})
                };
                let id = if tc.id.is_empty() {
                    format!("ollama-tool-{i}")
                } else {
                    tc.id.clone()
                };
                Some(ToolCall {
                    id,
                    name: tc.function.name.clone(),
                    arguments,
                })
            })
            .collect()
    }

    pub(crate) fn append_content_delta(out: &mut ChatResponse, piece: &str, sink: Option<&EventSink>) {
        if piece.is_empty() {
            return;
        }
        // Ollama may stream incremental tokens or cumulative text — handle both.
        // A cumulative chunk is STRICTLY LONGER than what we have and contains it as a
        // prefix. We must require strictly-longer: otherwise a normal incremental token
        // that happens to equal or prefix the accumulated tail (e.g. the second "2" of
        // "22", or a repeated digit in "443"/"8446") is misread as a duplicate and
        // dropped — silently clipping characters from replies.
        let delta = if out.content.is_empty() {
            piece.to_string()
        } else if piece.len() > out.content.len() && piece.starts_with(&out.content) {
            piece[out.content.len()..].to_string()
        } else {
            piece.to_string()
        };
        if delta.is_empty() {
            return;
        }
        out.content.push_str(&delta);
        emit(sink, StreamEvent::Text(delta));
    }

    async fn chat_non_stream(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String> {
        let url = join_url(&self.base_url, "api/chat");
        let body = self.build_body(req, false);

        let resp = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("ollama request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("ollama error {status}: {text}"));
        }

        #[derive(Debug, Deserialize)]
        struct OllamaChatResponse {
            #[serde(default)]
            message: OllamaMessage,
            #[serde(default)]
            done_reason: Option<String>,
            #[serde(default)]
            eval_count: Option<u64>,
            #[serde(default)]
            eval_duration: Option<u64>,
            #[serde(default)]
            prompt_eval_count: Option<u64>,
        }

        let parsed: OllamaChatResponse = resp
            .json()
            .await
            .map_err(|e| format!("ollama response parse error: {e}"))?;

        let mut out = ChatResponse::default();
        out.content = parsed.message.content.clone();
        if !out.content.is_empty() {
            emit(sink, StreamEvent::Text(out.content.clone()));
        }
        if let Some(ev) = stats_event(parsed.eval_count, parsed.eval_duration, parsed.prompt_eval_count) {
            emit(sink, ev);
        }
        for call in Self::parse_tool_calls(&parsed.message) {
            emit(sink, StreamEvent::ToolCall(call.clone()));
            out.tool_calls.push(call);
        }
        out.stop_reason = parsed.done_reason.unwrap_or_else(|| "stop".into());
        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    message: OllamaMessage,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    eval_count: Option<u64>,
    #[serde(default)]
    eval_duration: Option<u64>,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
}

/// Build a Stats event from Ollama's eval counters, when both are present and
/// non-zero. Single source for the non-stream and stream-done emit sites.
fn stats_event(
    eval_count: Option<u64>,
    eval_duration: Option<u64>,
    prompt_eval_count: Option<u64>,
) -> Option<StreamEvent> {
    let (count, dur_ns) = (eval_count?, eval_duration?);
    if count == 0 || dur_ns == 0 {
        return None;
    }
    let tps = count as f32 / (dur_ns as f32 / 1_000_000_000.0);
    Some(StreamEvent::Stats(StreamStats {
        completion_tokens: count as u32,
        prompt_tokens: prompt_eval_count.map(|n| n as u32),
        duration_ms: (dur_ns / 1_000_000).max(1),
        tokens_per_sec: tps,
    }))
}

#[derive(Debug, Default, Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCall {
    #[serde(default)]
    id: String,
    #[serde(default)]
    function: OllamaFunction,
}

#[derive(Debug, Default, Deserialize)]
struct OllamaFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[async_trait]
impl Provider for OllamaProvider {
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String> {
        let url = join_url(&self.base_url, "api/chat");
        let body = self.build_body(req, true);

        let resp = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("ollama request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("ollama error {status}: {text}"));
        }

        let mut out = ChatResponse::default();
        let mut line_buf = String::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            // User pressed Stop — abort the in-flight response immediately.
            if req.is_cancelled() {
                emit(sink, StreamEvent::Status("Stopped.".into()));
                break;
            }
            let chunk = chunk.map_err(|e| format!("ollama stream error: {e}"))?;
            line_buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = line_buf.find('\n') {
                let line: String = line_buf.drain(..=pos).collect();
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let chunk: OllamaStreamChunk = match serde_json::from_str(line) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                if !chunk.message.content.is_empty() {
                    Self::append_content_delta(&mut out, &chunk.message.content, sink);
                }

                if !chunk.message.thinking.is_empty() && self.options.think {
                    emit(
                        sink,
                        StreamEvent::Status(format!("Thinking: {}", chunk.message.thinking)),
                    );
                }

                if chunk.done {
                    if let Some(ev) =
                        stats_event(chunk.eval_count, chunk.eval_duration, chunk.prompt_eval_count)
                    {
                        emit(sink, ev);
                    }
                    if let Some(reason) = chunk.done_reason {
                        out.stop_reason = reason.clone();
                        if reason == "length" && out.content.len() < 32 {
                            emit(
                                sink,
                                StreamEvent::Status(
                                    "Model hit token/context limit — try 64K+ context in Settings → Providers."
                                        .into(),
                                ),
                            );
                        }
                    }
                    if out.content.is_empty() && !chunk.message.content.is_empty() {
                        Self::append_content_delta(&mut out, &chunk.message.content, sink);
                    }
                    for call in Self::parse_tool_calls(&chunk.message) {
                        emit(sink, StreamEvent::ToolCall(call.clone()));
                        out.tool_calls.push(call);
                    }
                }
            }
        }

        if out.content.trim().is_empty() && out.tool_calls.is_empty() {
            emit(
                sink,
                StreamEvent::Status("Retrying with non-streaming Ollama request…".into()),
            );
            return self.chat_non_stream(req, sink).await;
        }

        if out.stop_reason.is_empty() {
            out.stop_reason = "stop".into();
        }
        Ok(out)
    }
}
