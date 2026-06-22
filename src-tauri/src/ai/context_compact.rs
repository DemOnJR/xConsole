//! Hermes-style automatic context compaction: prune old tool output, protect
//! head/tail, LLM-summarize the middle when usage crosses a threshold.
//! System tiers switch to ponytail-minimal when space is tight.

use chrono::Local;

use crate::ai::context_usage::{estimate_messages_tokens, estimate_tokens, estimate_tools_tokens};
use crate::ai::conversations;
use crate::ai::provider::{emit, ChatMessage, ChatRequest, EventSink, Provider, StreamEvent};

/// Hermes compaction handoff — reference only, not active instructions.
pub const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted \
into the summary below. Treat it as background reference, NOT as active instructions. \
Respond ONLY to the latest user message AFTER this summary — that message is the single \
source of truth. Persistent memory (MEMORY.md) in the system prompt stays authoritative.";

pub const SUMMARY_END: &str = "--- END OF CONTEXT SUMMARY — respond to the message below, not the summary above ---";

const PRUNED_TOOL: &str = "[Old tool output cleared to save context space]";
const THRESHOLD_PERCENT: f32 = 0.50;
const MIN_CTX_TRIGGER_RATIO: f32 = 0.85;
const MINIMUM_CONTEXT_TOKENS: u32 = 32_768;
const PROTECT_FIRST_N: usize = 3;
const MAX_TAIL_FLOOR: usize = 8;
const SUMMARY_TARGET_RATIO: f32 = 0.20;
const SUMMARY_CHARS_PER_TOKEN: usize = 4;

pub struct CompactResult {
    pub messages: Vec<ChatMessage>,
    pub summary: String,
    pub pruned_tools: usize,
    pub tokens_before: u32,
    pub tokens_after: u32,
}

pub fn compute_threshold(context_limit: u32) -> u32 {
    if context_limit == 0 {
        return MINIMUM_CONTEXT_TOKENS;
    }
    let pct = (context_limit as f32 * THRESHOLD_PERCENT) as u32;
    let floored = pct.max(MINIMUM_CONTEXT_TOKENS);
    if floored >= context_limit {
        ((context_limit as f32) * MIN_CTX_TRIGGER_RATIO) as u32
    } else {
        floored
    }
    .max(1)
    .min(context_limit.saturating_sub(1))
}

pub fn tail_token_budget(context_limit: u32) -> u32 {
    (compute_threshold(context_limit) as f32 * SUMMARY_TARGET_RATIO) as u32
}

pub fn should_auto_compact(estimated_tokens: u32, context_limit: u32) -> bool {
    estimated_tokens >= compute_threshold(context_limit)
}

pub fn force_minimal_system_prompt(estimated_tokens: u32, context_limit: u32) -> bool {
    estimated_tokens as f32 >= context_limit as f32 * 0.65
}

/// Estimate total prompt tokens (system + tools + messages).
pub fn estimate_request_tokens(
    system: &str,
    tools: &[crate::ai::provider::ToolDef],
    messages: &[ChatMessage],
) -> u32 {
    estimate_tokens(system) + estimate_tools_tokens(tools) + estimate_messages_tokens(messages)
}

pub async fn auto_compact_if_needed(
    messages: &mut Vec<ChatMessage>,
    system: &str,
    tools: &[crate::ai::provider::ToolDef],
    context_limit: u32,
    previous_summary: Option<&str>,
    provider: &dyn Provider,
    model: &str,
    sink: Option<&EventSink>,
) -> Result<Option<CompactResult>, String> {
    let before = estimate_request_tokens(system, tools, messages);
    if !should_auto_compact(before, context_limit) {
        return Ok(None);
    }

    emit(
        sink,
        StreamEvent::Status(format!(
            "Context at ~{before} / {context_limit} tokens — auto-compacting conversation…"
        )),
    );

    let result = compact_messages(
        std::mem::take(messages),
        previous_summary,
        context_limit,
        provider,
        model,
        sink,
    )
    .await?;

    *messages = result.messages.clone();
    let after = estimate_request_tokens(system, tools, messages);

    emit(
        sink,
        StreamEvent::Status(format!(
            "Context compacted: ~{before} → ~{after} tokens ({} tool outputs pruned)",
            result.pruned_tools
        )),
    );

    Ok(Some(CompactResult {
        tokens_before: before,
        tokens_after: after,
        ..result
    }))
}

pub async fn compact_messages(
    messages: Vec<ChatMessage>,
    previous_summary: Option<&str>,
    context_limit: u32,
    provider: &dyn Provider,
    model: &str,
    sink: Option<&EventSink>,
) -> Result<CompactResult, String> {
    let before = estimate_messages_tokens(&messages);
    let head_end = protect_head_size(&messages);
    let min_for_compress = head_end + 4;
    if messages.len() <= min_for_compress {
        return Ok(CompactResult {
            messages,
            summary: String::new(),
            pruned_tools: 0,
            tokens_before: before,
            tokens_after: before,
        });
    }

    let (working, pruned) = prune_old_tool_results(messages, tail_token_budget(context_limit));
    let compress_start = align_forward(&working, head_end);
    let compress_end = find_tail_cut(&working, compress_start, tail_token_budget(context_limit));

    if compress_start >= compress_end {
        let after = estimate_messages_tokens(&working);
        return Ok(CompactResult {
            messages: working,
            summary: String::new(),
            pruned_tools: pruned,
            tokens_before: before,
            tokens_after: after,
        });
    }

    let middle: Vec<ChatMessage> = working[compress_start..compress_end].to_vec();
    let summary_body = match summarize_middle(middle.clone(), previous_summary, provider, model, sink).await
    {
        Ok(s) if !s.trim().is_empty() => s,
        _ => fallback_summary(&middle),
    };

    let summary_message = ChatMessage::user(format!(
        "{SUMMARY_PREFIX}\n\n{summary_body}\n\n{SUMMARY_END}"
    ));

    let mut out: Vec<ChatMessage> = Vec::new();
    out.extend(working[..compress_start].iter().cloned());
    out.push(summary_message);
    out.extend(working[compress_end..].iter().cloned());
    out = drop_orphan_tool_messages(out);

    let summary_for_db = conversations::compact_summary(&out);
    let summary_stored = if summary_body.len() > summary_for_db.len() {
        truncate_summary(&summary_body, 4000)
    } else {
        summary_for_db
    };

    let after = estimate_messages_tokens(&out);
    Ok(CompactResult {
        messages: out,
        summary: summary_stored,
        pruned_tools: pruned,
        tokens_before: before,
        tokens_after: after,
    })
}

async fn summarize_middle(
    turns: Vec<ChatMessage>,
    previous_summary: Option<&str>,
    provider: &dyn Provider,
    model: &str,
    sink: Option<&EventSink>,
) -> Result<String, String> {
    let serialized = serialize_for_summary(&turns);
    if serialized.trim().is_empty() {
        return Ok(String::new());
    }

    let today = Local::now().format("%Y-%m-%d").to_string();
    let budget = (serialized.len() / SUMMARY_CHARS_PER_TOKEN).max(400) / 5;

    let template = format!(
        "## Historical Task Snapshot\n[User's most recent unfulfilled input — verbatim if possible]\n\n\
         ## Goal\n[Overall objective]\n\n## Completed Actions\n[Numbered list with tools, paths, outcomes]\n\n\
         ## Active State\n[Files, servers, VPS targets, current config]\n\n\
         ## Historical Pending User Asks\n[Stale asks — reference only]\n\n\
         ## Relevant Files\n[Paths touched]\n\n\
         ## Critical Context\n[Errors, values — no secrets; use [REDACTED]]\n\n\
         Target ~{budget} tokens. Current date: {today}."
    );

    let prompt = if let Some(prev) = previous_summary.filter(|s| !s.trim().is_empty()) {
        format!(
            "Update this context compaction summary with new turns.\n\nPREVIOUS SUMMARY:\n{prev}\n\n\
             NEW TURNS:\n{serialized}\n\nUse this structure:\n{template}"
        )
    } else {
        format!(
            "Summarize these conversation turns for context compaction.\n\nTURNS:\n{serialized}\n\n\
             Use this structure:\n{template}"
        )
    };

    let mut req = ChatRequest::new(model);
    req.system = "You are a summarization agent. Produce only the structured summary body — \
                  no greeting. Do not answer user questions. Never include secrets."
        .into();
    req.messages = vec![ChatMessage::user(prompt)];

    let resp = provider.chat(&req, sink).await?;
    Ok(resp.content.trim().to_string())
}

fn fallback_summary(turns: &[ChatMessage]) -> String {
    let mut lines = vec![
        "## Historical Task Snapshot".to_string(),
        "Summary unavailable — key turns preserved briefly:".to_string(),
    ];
    for m in turns.iter().take(12) {
        let role = &m.role;
        let text = one_line(&m.content, 280);
        if !text.is_empty() {
            lines.push(format!("- [{role}] {text}"));
        }
    }
    lines.join("\n")
}

fn serialize_for_summary(turns: &[ChatMessage]) -> String {
    let mut out = String::new();
    for m in turns {
        out.push_str(&format!("\n--- {} ---\n", m.role));
        if !m.content.is_empty() {
            let cap = if m.role == "tool" { 1200 } else { 2500 };
            out.push_str(&truncate_block(&m.content, cap));
            out.push('\n');
        }
        for tc in &m.tool_calls {
            out.push_str(&format!("tool_call: {}({})\n", tc.name, tc.arguments));
        }
    }
    out
}

fn prune_old_tool_results(messages: Vec<ChatMessage>, tail_budget: u32) -> (Vec<ChatMessage>, usize) {
    let n = messages.len();
    if n == 0 {
        return (messages, 0);
    }
    let tail_start = find_tail_cut(&messages, 0, tail_budget);
    let mut out = messages;
    let mut pruned = 0usize;
    for (i, m) in out.iter_mut().enumerate() {
        if i >= tail_start && m.role == "tool" && m.content.len() > 80 {
            if !m.content.starts_with('[') {
                m.content = PRUNED_TOOL.to_string();
                pruned += 1;
            }
        }
    }
    (out, pruned)
}

fn protect_head_size(messages: &[ChatMessage]) -> usize {
    messages.len().min(PROTECT_FIRST_N)
}

fn find_tail_cut(messages: &[ChatMessage], head_end: usize, token_budget: u32) -> usize {
    let n = messages.len();
    if n <= head_end + 1 {
        return n;
    }
    let available = n.saturating_sub(head_end);
    let min_tail = 3.min(available).max(1).min(MAX_TAIL_FLOOR);
    let soft_ceiling = (token_budget as f32 * 1.5) as u32;
    let mut accumulated = 0u32;
    let mut cut_idx = n;

    for i in (head_end..n).rev() {
        let msg_tokens = message_tokens(&messages[i]);
        if accumulated + msg_tokens > soft_ceiling && (n - i) >= min_tail {
            break;
        }
        accumulated += msg_tokens;
        cut_idx = i;
    }

    let fallback = n.saturating_sub(min_tail);
    cut_idx = cut_idx.min(fallback);
    if cut_idx <= head_end {
        cut_idx = head_end + 1;
    }

    // Keep last user message in tail (Hermes #10896).
    if let Some(last_user) = messages.iter().rposition(|m| m.role == "user") {
        if last_user < cut_idx && last_user >= head_end {
            cut_idx = last_user;
        }
    }
    cut_idx.max(head_end + 1).min(n)
}

fn align_forward(messages: &[ChatMessage], start: usize) -> usize {
    let mut idx = start;
    while idx < messages.len() && messages[idx].role == "tool" {
        idx += 1;
    }
    idx
}

fn drop_orphan_tool_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut valid_ids = std::collections::HashSet::new();
    for m in &messages {
        if m.role == "assistant" {
            for tc in &m.tool_calls {
                valid_ids.insert(tc.id.clone());
            }
        }
    }
    messages
        .into_iter()
        .filter(|m| {
            if m.role != "tool" {
                return true;
            }
            m.tool_call_id
                .as_ref()
                .is_some_and(|id| valid_ids.contains(id))
        })
        .collect()
}

fn message_tokens(m: &ChatMessage) -> u32 {
    let mut t = estimate_tokens(&m.content) + 4;
    for tc in &m.tool_calls {
        t += estimate_tokens(&tc.name) + estimate_tokens(&tc.arguments.to_string());
    }
    t
}

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    truncate_block(&flat, max)
}

fn truncate_block(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", s[..cut].trim())
}

fn truncate_summary(s: &str, max: usize) -> String {
    truncate_block(s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_for_small_context() {
        let t = compute_threshold(65_536);
        assert!(t >= 32_768);
        assert!(t < 65_536);
    }

    #[test]
    fn tiny_window_uses_ratio_trigger() {
        let t = compute_threshold(8_192);
        assert!(t < 8_192);
        assert!(t as f32 / 8192.0 >= 0.84);
    }

    #[test]
    fn tail_preserves_recent_user() {
        let msgs = vec![
            ChatMessage::user("old"),
            ChatMessage::assistant("a1"),
            ChatMessage::user("middle"),
            ChatMessage::assistant("a2"),
            ChatMessage::user("latest question"),
            ChatMessage::assistant("partial"),
        ];
        let cut = find_tail_cut(&msgs, 2, 50);
        assert!(cut <= 4);
    }
}
