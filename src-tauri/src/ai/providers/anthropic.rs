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
    /// There is no such thing as an "implicit prefix" behind the system and
    /// tool breakpoints: Anthropic caches the request only up to the *last*
    /// `cache_control` marker, so with markers only on tools and system, every
    /// message in the conversation is re-billed at full price on every request.
    /// A 15-iteration tool loop paid for the whole transcript 15 times.
    ///
    /// So this puts the remaining two of the four allowed breakpoints on the
    /// message list, using the documented multi-turn pattern:
    ///
    /// * **BP3** on the last content block of the *previous* user turn — a
    ///   slow-moving anchor. It stays byte-identical while the tail grows, so
    ///   the next request reads everything up to it.
    /// * **BP4** on the last content block of the last message — the growing
    ///   edge, so the turn we just paid to write is readable next time.
    ///
    /// Breakpoints are *read points*, not exclusions: a block that carried a
    /// marker on an earlier request is still a cache hit after the marker
    /// moves on. Total markers per request: tools(1) + system(1) + 2 = 4, the
    /// documented maximum.
    fn build_messages(messages: &[ChatMessage]) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for m in messages {
            match m.role.as_str() {
                "user" => {
                    // Always a block array, never a bare string. A breakpoint
                    // cannot sit on a string, so marking one would otherwise
                    // change a message's *shape* the turn it becomes BP3/BP4 and
                    // change it back the turn after — a needless difference in a
                    // prefix that has to stay byte-stable.
                    let content =
                        crate::ai::vision::anthropic_user_content(&m.content, &m.images);
                    let blocks = match content {
                        Value::String(text) => json!([{"type": "text", "text": text}]),
                        other => other,
                    };
                    out.push(json!({"role": "user", "content": blocks}));
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
                    // Tool results are user-role blocks in Anthropic's schema, and
                    // the API wants *all* of a parallel batch in ONE user message.
                    // Splitting them across messages teaches the model to stop
                    // calling tools in parallel, and each extra message is another
                    // position against the 20-block cache lookback window.
                    let block = json!({
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content,
                    });
                    match out.last_mut() {
                        Some(prev)
                            if prev["role"] == "user"
                                && prev["content"]
                                    .as_array()
                                    .is_some_and(|b| {
                                        b.last().is_some_and(|last| last["type"] == "tool_result")
                                    }) =>
                        {
                            prev["content"].as_array_mut().unwrap().push(block);
                        }
                        _ => out.push(json!({"role": "user", "content": [block]})),
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Mark the two message-level cache breakpoints (BP3 + BP4).
    ///
    /// `ttl` must match the TTL used on the tools/system markers: entries with a
    /// longer TTL have to appear *before* shorter ones, and mixing them within
    /// one request is how the previous code silently ran a 5-minute tools cache
    /// in front of a 1-hour system cache.
    fn mark_message_breakpoints(out: &mut [Value], ttl: crate::ai::cost::CacheTtl) {
        if out.is_empty() {
            return;
        }
        let last = out.len() - 1;
        // BP3: last content block of the previous *real* user turn.
        //
        // Tool results are user-role blocks in this schema, so "the last user
        // message" would move on every loop iteration and be no anchor at all.
        // A user message carrying no `tool_result` block changes once per user
        // turn — including the frozen trailing runtime block — so it stays a
        // valid read point for the whole tool loop, behind the moving BP4.
        if let Some(prev_user) = out[..last].iter().rposition(|m| {
            m["role"] == "user"
                && !m["content"]
                    .as_array()
                    .is_some_and(|b| b.iter().any(|x| x["type"] == "tool_result"))
        }) {
            Self::mark_last_block(&mut out[prev_user], ttl);
        }
        // BP4: last content block of the last message (the growing edge).
        Self::mark_last_block(&mut out[last], ttl);
    }

    /// Put `cache_control` on a message's final content block, promoting a bare
    /// string body to a one-element block array first (a string cannot carry a
    /// marker).
    fn mark_last_block(message: &mut Value, ttl: crate::ai::cost::CacheTtl) {
        if let Some(text) = message["content"].as_str() {
            if text.is_empty() {
                return;
            }
            message["content"] = json!([{"type": "text", "text": text}]);
        }
        let Some(blocks) = message["content"].as_array_mut() else {
            return;
        };
        let Some(last) = blocks.last_mut() else {
            return;
        };
        last["cache_control"] = Self::cache_control(ttl);
    }

    /// Assemble the whole `/v1/messages` request body.
    ///
    /// Kept out of `chat` so the exact bytes we send — including every
    /// `cache_control` marker — are assertable in tests and in the bench.
    pub fn build_body(req: &ChatRequest) -> Value {
        // One TTL for every breakpoint in the request. Entries with the longer
        // TTL must appear before shorter ones, and tools render before system —
        // the old code marked tools at 5m and system at 1h, which is that rule
        // backwards.
        let ttl = req.cache_ttl();
        let mut messages = Self::build_messages(&req.messages);
        Self::mark_message_breakpoints(&mut messages, ttl);
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "stream": true,
            "messages": messages,
        });
        // Sampling parameters were removed on Opus 4.7+/Opus 5/Sonnet 5/Fable:
        // sending `temperature` there is a 400, not a no-op.
        if accepts_sampling(&req.model) {
            body["temperature"] =
                json!(crate::ai::provider::format_temperature(req.temperature, 0.0, 1.0));
        }
        apply_thinking(&mut body, &req.model, &req.reasoning);
        // Prompt caching: mark the static system prefix so multi-turn agent loops
        // reuse Anthropic's server-side cache.
        if !req.system.is_empty() {
            body["system"] = json!([{
                "type": "text",
                "text": req.system,
                "cache_control": Self::cache_control(ttl),
            }]);
        }
        let tools = Self::build_tools(req);
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        body
    }

    /// A `cache_control` value for the requested TTL.
    fn cache_control(ttl: crate::ai::cost::CacheTtl) -> Value {
        match ttl {
            crate::ai::cost::CacheTtl::OneHour => json!({"type": "ephemeral", "ttl": "1h"}),
            crate::ai::cost::CacheTtl::FiveMinutes => json!({"type": "ephemeral"}),
        }
    }

    fn build_tools(req: &ChatRequest) -> Vec<Value> {
        let ttl = req.cache_ttl();
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
                // everything up to the last breakpoint). The TTL must match the one
                // used on system and the message breakpoints — a 1h entry has to
                // come before any 5m entry, and tools render first.
                if i == count - 1 {
                    def["cache_control"] = Self::cache_control(ttl);
                }
                def
            })
            .collect()
    }
}

/// Model families that took adaptive thinking and dropped `budget_tokens`.
///
/// Verified against the current API contract, not from memory:
/// `thinking: {type: "enabled", budget_tokens: N}` is **rejected with a 400** on
/// Fable 5/5.1, Mythos 5/5.1, Opus 5, Opus 4.8, Opus 4.7 and Sonnet 5, and is
/// deprecated on Opus 4.6 / Sonnet 4.6. Depth is controlled by
/// `output_config.effort` instead. Pre-4.6 models still take `budget_tokens`.
fn uses_adaptive_thinking(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("fable")
        || m.contains("mythos")
        || m.contains("opus-5")
        || m.contains("opus-4-8")
        || m.contains("opus-4-7")
        || m.contains("opus-4-6")
        || m.contains("sonnet-5")
        || m.contains("sonnet-4-6")
}

/// Models that still accept `temperature` / `top_p` / `top_k`.
///
/// Sampling parameters were removed on Opus 4.7 and later, Opus 5, Sonnet 5 and
/// the Fable/Mythos family — sending one is a 400. Opus 4.6 and Sonnet 4.6 still
/// take them, as does everything older.
fn accepts_sampling(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    !(m.contains("fable")
        || m.contains("mythos")
        || m.contains("opus-5")
        || m.contains("opus-4-8")
        || m.contains("opus-4-7")
        || m.contains("sonnet-5"))
}

/// Map our `agent.reasoning_level` setting onto the model's actual contract.
///
/// Note on `off`: on Opus 5 an explicit `thinking: {type: "disabled"}` makes the
/// model occasionally write a tool call into its *visible text* instead of a
/// `tool_use` block — the turn succeeds, the call never runs, and nothing errors.
/// In a tool loop that is a silent correctness bug, so "off" is honoured as
/// adaptive thinking at the lowest effort, which is both cheaper and safe.
///
/// Thinking and effort changes invalidate the messages cache, so these are read
/// once per turn (see `agent.rs`) and never varied mid-loop.
fn apply_thinking(body: &mut Value, model: &str, reasoning: &str) {
    let level = reasoning.trim().to_ascii_lowercase();
    if level.is_empty() {
        return; // provider default
    }
    if uses_adaptive_thinking(model) {
        let effort = match level.as_str() {
            "off" | "low" => "low",
            "medium" => "medium",
            "xhigh" => "xhigh",
            "max" => "max",
            _ => "high",
        };
        body["thinking"] = json!({"type": "adaptive"});
        body["output_config"] = json!({"effort": effort});
        return;
    }
    if level == "off" {
        return;
    }
    // Pre-4.6: the fixed thinking budget is still the only knob.
    let budget = match level.as_str() {
        "low" => 2048,
        "high" => 16384,
        _ => 8192,
    };
    body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
    // Thinking requires temperature 1 on these models.
    body["temperature"] = json!(1.0);
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String> {
        let url = join_url(&self.base_url, "v1/messages");
        let ttl = req.cache_ttl();
        let body = Self::build_body(req);

        // Prompt caching and the 1-hour TTL are both GA — `prompt-caching-2024-07-31`
        // and `extended-cache-ttl-2025-01-23` are retired beta flags and no longer
        // gate anything.
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
                            cache_write_tokens = u
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32)
                                .or(cache_write_tokens);
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
        out.cache_creation_tokens = cache_write_tokens;
        out.completion_tokens = output_tokens;

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
                StreamEvent::Cost(crate::ai::cost::turn_cost_ttl(
                    "anthropic",
                    &req.model,
                    input_tokens,
                    completion,
                    cache_read_tokens,
                    cache_write_tokens,
                    ttl,
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

    /// Count every `cache_control` marker in a rendered request body.
    fn count_breakpoints(body: &Value) -> usize {
        fn walk(v: &Value, n: &mut usize) {
            match v {
                Value::Object(map) => {
                    if map.contains_key("cache_control") {
                        *n += 1;
                    }
                    for child in map.values() {
                        walk(child, n);
                    }
                }
                Value::Array(items) => {
                    for child in items {
                        walk(child, n);
                    }
                }
                _ => {}
            }
        }
        let mut n = 0;
        walk(body, &mut n);
        n
    }

    fn loop_request(model: &str) -> ChatRequest {
        let mut req = ChatRequest::new(model);
        req.system = "stable system prefix".into();
        req.tools = vec![
            ToolDef { name: "a".into(), description: "a".into(), parameters: json!({}) },
            ToolDef { name: "b".into(), description: "b".into(), parameters: json!({}) },
        ];
        req.messages = vec![
            ChatMessage::user("first user turn"),
            ChatMessage::assistant("thinking"),
            ChatMessage::tool_result("call-1", "output"),
            ChatMessage::user("second user turn"),
            ChatMessage::assistant("more"),
            ChatMessage::tool_result("call-2", "more output"),
        ];
        req
    }

    #[test]
    fn last_message_block_carries_cache_control() {
        // The bug this replaces: history carried no `cache_control` at all, so a
        // 15-iteration tool loop re-billed the whole conversation 15 times.
        let req = loop_request("claude-opus-5");
        let body = AnthropicProvider::build_body(&req);
        let msgs = body["messages"].as_array().unwrap();

        // BP4: last content block of the last message.
        let last = msgs.last().unwrap();
        let last_block = last["content"].as_array().unwrap().last().unwrap();
        assert_eq!(
            last_block["cache_control"]["type"], "ephemeral",
            "the growing edge must be a read point next turn: {last}"
        );

        // BP3: the previous *real* user turn — not a tool_result, which is also
        // user-role here and would move on every loop iteration.
        let prev_user = msgs[..msgs.len() - 1]
            .iter()
            .rposition(|m| {
                m["role"] == "user"
                    && !m["content"]
                        .as_array()
                        .is_some_and(|b| b.iter().any(|x| x["type"] == "tool_result"))
            })
            .expect("a previous real user turn");
        assert_eq!(msgs[prev_user]["content"][0]["text"], "second user turn");
        let anchor_block = msgs[prev_user]["content"].as_array().unwrap().last().unwrap();
        assert_eq!(anchor_block["cache_control"]["type"], "ephemeral");

        // Nothing else in the message list is marked.
        let marked: usize = msgs
            .iter()
            .filter(|m| {
                m["content"]
                    .as_array()
                    .is_some_and(|b| b.iter().any(|x| x.get("cache_control").is_some()))
            })
            .count();
        assert_eq!(marked, 2, "exactly BP3 and BP4 on the message list");
    }

    #[test]
    fn the_anchor_breakpoint_holds_still_across_a_tool_loop() {
        // Within one user turn the loop appends assistant + tool_result pairs.
        // BP4 moves with the tail (that is its job), but BP3 must stay pinned to
        // the same real user turn all the way through, so there is always a
        // second, older read point behind it.
        let mut req = ChatRequest::new("claude-opus-5");
        req.system = "stable".into();
        req.messages = vec![ChatMessage::user("do the thing")];

        let anchor_text = |req: &ChatRequest| -> Option<String> {
            let body = AnthropicProvider::build_body(req);
            let msgs = body["messages"].as_array().unwrap().clone();
            let last = msgs.len() - 1;
            msgs[..last].iter().find_map(|m| {
                let blocks = m["content"].as_array()?;
                blocks
                    .iter()
                    .any(|b| b.get("cache_control").is_some())
                    .then(|| blocks.last()?.get("text")?.as_str().map(str::to_string))
                    .flatten()
            })
        };

        let mut seen = Vec::new();
        for i in 0..5 {
            seen.push(anchor_text(&req));
            req.messages.push(ChatMessage::assistant(format!("step {i}")));
            req.messages
                .push(ChatMessage::tool_result(format!("c{i}"), "output"));
        }
        // Iteration 0 has no previous message at all; every later iteration
        // anchors on the same user turn.
        assert_eq!(seen[0], None);
        assert!(
            seen[1..].iter().all(|t| t.as_deref() == Some("do the thing")),
            "anchor moved during the tool loop: {seen:?}"
        );
    }

    #[test]
    fn a_request_never_exceeds_four_cache_breakpoints() {
        // The API rejects a fifth. tools(1) + system(1) + BP3 + BP4 == 4.
        for model in ["claude-opus-5", "claude-sonnet-4-6", "claude-3-5-haiku-latest"] {
            for retention in ["", "short", "long"] {
                let mut req = loop_request(model);
                req.cache_retention = retention.into();
                let n = count_breakpoints(&AnthropicProvider::build_body(&req));
                assert!(n <= 4, "{model}/{retention}: {n} breakpoints");
                assert_eq!(n, 4, "{model}/{retention}: expected all four to be used");
            }
        }
        // A single-message request has no previous user turn, so only 3.
        let mut solo = loop_request("claude-opus-5");
        solo.messages = vec![ChatMessage::user("only turn")];
        assert_eq!(count_breakpoints(&AnthropicProvider::build_body(&solo)), 3);
    }

    #[test]
    fn every_breakpoint_in_one_request_shares_a_ttl() {
        // A 1-hour entry must appear before any 5-minute entry, and tools render
        // before system: marking tools at 5m and system at 1h (the old code) is
        // that ordering rule backwards.
        let mut req = loop_request("claude-opus-5");
        req.cache_retention = "long".into();
        let body = AnthropicProvider::build_body(&req);
        let text = body.to_string();
        let total = count_breakpoints(&body);
        assert_eq!(text.matches("\"ttl\":\"1h\"").count(), total);

        let mut short = loop_request("claude-opus-5");
        short.cache_retention = "short".into();
        assert!(!AnthropicProvider::build_body(&short).to_string().contains("1h"));
    }

    #[test]
    fn autonomous_sessions_default_to_the_one_hour_ttl() {
        // A goal cycle spends minutes in tool work, so the 5-minute entry is
        // always expired by the next cycle and rewritten at 1.25x — worse than
        // not caching. An explicit setting still wins.
        let mut auto = loop_request("claude-opus-5");
        auto.autonomous = true;
        assert!(AnthropicProvider::build_body(&auto).to_string().contains("1h"));

        let mut pinned = loop_request("claude-opus-5");
        pinned.autonomous = true;
        pinned.cache_retention = "short".into();
        assert!(!AnthropicProvider::build_body(&pinned).to_string().contains("1h"));

        // Interactive turns arrive well under 5 minutes apart; every read
        // refreshes the entry for free, so the cheaper TTL stays the default.
        let chat = loop_request("claude-opus-5");
        assert!(!AnthropicProvider::build_body(&chat).to_string().contains("1h"));
    }

    #[test]
    fn image_turn_does_not_move_the_tools_breakpoint() {
        // Tools render at position 0. Registering the vision tool only on turns
        // that carry an image moved the tools breakpoint and forced two full
        // tool-block rewrites — one to add it, one to drop it again.
        let plain = loop_request("claude-opus-5");
        let mut with_image = loop_request("claude-opus-5");
        with_image.messages[0].images = vec![crate::ai::provider::ChatImage {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            data: "aGVsbG8=".into(),
        }];

        let tools_of = |req: &ChatRequest| AnthropicProvider::build_body(req)["tools"].clone();
        assert_eq!(
            tools_of(&plain),
            tools_of(&with_image),
            "an attached image must not change the tool block at all"
        );

        // And the image rides in the message content, ahead of its text.
        let body = AnthropicProvider::build_body(&with_image);
        let first = &body["messages"][0]["content"];
        assert_eq!(first[0]["type"], "image");
        assert_eq!(first[1]["type"], "text");
        // BP3/BP4 still land, and still only twice.
        assert_eq!(count_breakpoints(&body), 4);
    }

    #[test]
    fn parallel_tool_results_share_one_user_message() {
        // The API wants every `tool_result` of a batch in ONE user message;
        // splitting them trains the model out of parallel calls and burns
        // positions against the 20-block cache lookback window.
        let mut req = ChatRequest::new("claude-opus-5");
        req.messages = vec![
            ChatMessage::user("go"),
            ChatMessage::assistant("running"),
            ChatMessage::tool_result("c1", "one"),
            ChatMessage::tool_result("c2", "two"),
            ChatMessage::tool_result("c3", "three"),
        ];
        let msgs = AnthropicProvider::build_messages(&req.messages);
        assert_eq!(msgs.len(), 3, "user, assistant, one batched tool_result turn");
        let results = msgs[2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|b| b["type"] == "tool_result"));
    }

    #[test]
    fn thinking_matches_the_model_contract() {
        // `budget_tokens` is a 400 on Opus 5 / 4.8 / 4.7, Sonnet 5 and Fable;
        // so is any sampling parameter. Depth is `output_config.effort`.
        let mut modern = ChatRequest::new("claude-opus-5");
        modern.reasoning = "high".into();
        modern.messages = vec![ChatMessage::user("hi")];
        let body = AnthropicProvider::build_body(&modern);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "high");
        assert!(body.get("temperature").is_none(), "sampling is rejected on Opus 5");

        // "off" becomes low effort rather than `thinking: disabled`: with thinking
        // disabled, Opus 5 can write a tool call into its visible text instead of
        // a tool_use block — silent breakage in a tool loop.
        let mut off = ChatRequest::new("claude-opus-5");
        off.reasoning = "off".into();
        off.messages = vec![ChatMessage::user("hi")];
        let off_body = AnthropicProvider::build_body(&off);
        assert_eq!(off_body["thinking"]["type"], "adaptive");
        assert_eq!(off_body["output_config"]["effort"], "low");

        // Pre-4.6 models keep the fixed budget and require temperature 1.
        let mut legacy = ChatRequest::new("claude-3-5-sonnet-latest");
        legacy.reasoning = "high".into();
        legacy.messages = vec![ChatMessage::user("hi")];
        let legacy_body = AnthropicProvider::build_body(&legacy);
        assert_eq!(legacy_body["thinking"]["type"], "enabled");
        assert_eq!(legacy_body["thinking"]["budget_tokens"], 16384);
        assert_eq!(legacy_body["temperature"], 1.0);

        // Opus 4.6 takes adaptive thinking but still accepts sampling.
        let mut mid = ChatRequest::new("claude-opus-4-6");
        mid.reasoning = "medium".into();
        mid.messages = vec![ChatMessage::user("hi")];
        let mid_body = AnthropicProvider::build_body(&mid);
        assert_eq!(mid_body["thinking"]["type"], "adaptive");
        assert!(mid_body.get("temperature").is_some());

        // No setting at all leaves the provider default untouched.
        let mut none = ChatRequest::new("claude-opus-5");
        none.messages = vec![ChatMessage::user("hi")];
        let none_body = AnthropicProvider::build_body(&none);
        assert!(none_body.get("thinking").is_none());
        assert!(none_body.get("output_config").is_none());
    }
}
