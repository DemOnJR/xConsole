//! Estimate how the agent context window is filled before each model call.

use serde::Serialize;

use crate::ai::context::PromptContext;
use crate::ai::provider::{ChatMessage, ToolDef};

/// One segment of the prompt, with an estimated token count (~4 chars/token).
#[derive(Debug, Clone, Serialize)]
pub struct ContextSegment {
    pub key: String,
    pub label: String,
    pub tokens: u32,
}

/// Breakdown emitted to the UI (Cursor / OpenCode style).
#[derive(Debug, Clone, Serialize)]
pub struct ContextUsage {
    pub segments: Vec<ContextSegment>,
    pub total_tokens: u32,
    pub context_limit: u32,
    pub percent: f32,
}

pub fn estimate_tokens(text: &str) -> u32 {
    crate::ai::text::count_tokens(text) as u32
}

pub fn estimate_tools_tokens(tools: &[ToolDef]) -> u32 {
    tools
        .iter()
        .map(|t| {
            estimate_tokens(&t.name)
                + estimate_tokens(&t.description)
                + estimate_tokens(&t.parameters.to_string())
        })
        .sum()
}

pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> u32 {
    let mut total = 0u32;
    for m in messages {
        total += estimate_tokens(&m.content);
        for tc in &m.tool_calls {
            total += estimate_tokens(&tc.name);
            total += estimate_tokens(&tc.arguments.to_string());
        }
        if let Some(id) = &m.tool_call_id {
            total += estimate_tokens(id);
        }
        // per-message overhead (role markers)
        total += 4;
    }
    total
}

/// Context window for a Claude model, or `None` when the name is not a Claude
/// model we recognise.
///
/// Opus 4.6 and later, Sonnet 4.6 and later, and the Fable/Mythos family are all
/// **1M**; only Haiku and the claude-3.x generation are still 200K. Treating the
/// whole vendor as 200K flipped the minimal system prompt at 130K and fired
/// compaction at 160K on models with five times that room — and each of those
/// rewrites the entire tools + system + messages prefix.
pub fn claude_context_limit(model: &str) -> Option<u32> {
    let m = model.to_lowercase();
    if !(m.contains("claude") || m.contains("opus") || m.contains("sonnet")
        || m.contains("haiku") || m.contains("fable") || m.contains("mythos"))
    {
        return None;
    }
    // 200K holdouts first: Haiku (every generation) and the claude-3.x line.
    if m.contains("haiku") || m.contains("claude-3") {
        return Some(200_000);
    }
    if m.contains("fable")
        || m.contains("mythos")
        || m.contains("opus-5")
        || m.contains("opus-4-8")
        || m.contains("opus-4-7")
        || m.contains("opus-4-6")
        || m.contains("sonnet-5")
        || m.contains("sonnet-4-6")
    {
        return Some(1_000_000);
    }
    // Opus 4.5 / Sonnet 4.5 and anything else Claude-shaped we do not know.
    Some(200_000)
}

/// Default context window for providers without an explicit setting.
///
/// DeepSeek V4 Flash/Pro advertise 1M. Treating them as 128K made auto-compact
/// fire around 64K and rewrite the cached prefix — the opposite of a 95%+
/// long-session hit rate.
pub fn default_context_limit(
    provider_kind: &str,
    model: &str,
    ollama_num_ctx: Option<u32>,
) -> u32 {
    let m = model.to_lowercase();
    if m.contains("deepseek") || m.contains("ox-alpha") || m.contains("0x-alpha") || m.contains("stealth") {
        return 1_048_576;
    }
    if provider_kind == "ollama" {
        return ollama_num_ctx.unwrap_or(65_536);
    }
    // A Claude model is 1M or 200K by name, whichever adapter is carrying it.
    if let Some(limit) = claude_context_limit(&m) {
        return limit;
    }
    match provider_kind {
        "anthropic" => 200_000,
        "cursor" | "codex_cli" | "opencode_cli" | "antigravity_cli" => 200_000,
        _ => 128_000,
    }
}

/// Build a usage report from the assembled prompt pieces for this turn.
pub fn compute_usage(
    ctx: &PromptContext<'_>,
    tools: &[ToolDef],
    messages: &[ChatMessage],
    vps_snapshot: &str,
    live_command: &str,
    provider_kind: &str,
) -> ContextUsage {
    let parts = crate::ai::context::measure_prompt_parts(ctx);
    let tool_tokens = estimate_tools_tokens(tools);
    let conversation_tokens = estimate_messages_tokens(messages);
    let vps_prefetch_tokens = estimate_tokens(vps_snapshot) + estimate_tokens(live_command);
    let runtime_tokens = estimate_runtime(ctx);

    let mut segments = vec![
        segment("system_prompt", "Runtime", runtime_tokens),
        segment("rules", "Rules", parts.rules_tokens),
        segment("tool_definitions", "Tool definitions", tool_tokens),
        segment("skills", "Skills", parts.skills_tokens),
        segment("memory", "Memory", parts.memory_tokens),
        segment("infra", "Infra inventory", parts.infra_tokens),
        segment("vps_prefetch", "VPS prefetch", vps_prefetch_tokens),
        segment(
            "conversation_summary",
            "Summarized conversation",
            parts.summary_tokens,
        ),
        segment("conversation", "Conversation", conversation_tokens),
    ];
    segments.retain(|s| s.tokens > 0);

    let total_tokens: u32 = segments.iter().map(|s| s.tokens).sum();
    let context_limit = default_context_limit(provider_kind, ctx.model_label, ctx.ollama_num_ctx);
    let percent = if context_limit > 0 {
        ((total_tokens as f64 / context_limit as f64) * 100.0).min(100.0) as f32
    } else {
        0.0
    };

    ContextUsage {
        segments,
        total_tokens,
        context_limit,
        percent,
    }
}

#[cfg(test)]
mod tests {
    use super::default_context_limit;

    #[test]
    fn deepseek_flash_uses_one_million_context() {
        assert_eq!(
            default_context_limit("openai", "deepseek/deepseek-v4-flash", None),
            1_048_576
        );
        assert_eq!(default_context_limit("openai", "gpt-5.6", None), 128_000);
    }

    #[test]
    fn current_claude_models_are_one_million_not_two_hundred_thousand() {
        for m in [
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-6",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-fable-5-1",
        ] {
            assert_eq!(default_context_limit("anthropic", m, None), 1_000_000, "{m}");
        }
        // Haiku and the 3.x line really are 200K.
        assert_eq!(default_context_limit("anthropic", "claude-haiku-4-5", None), 200_000);
        assert_eq!(default_context_limit("anthropic", "claude-3-5-sonnet", None), 200_000);
        // The window follows the model name through an openai-compat adapter too.
        assert_eq!(default_context_limit("openai", "anthropic/claude-opus-5", None), 1_000_000);
    }
}

fn estimate_runtime(ctx: &PromptContext<'_>) -> u32 {
    let mut runtime = format!("Date: {}", chrono::Local::now().format("%A, %B %d, %Y"));
    if !ctx.casual_turn {
        runtime.push_str(&format!(
            "\nReachable VPS targets this session: {}",
            ctx.target_count
        ));
    }
    if ctx.target_count == 0 {
        runtime.push_str(if ctx.vps_tools_only {
            "\nNo VPS targets selected: SSH tools unavailable this turn."
        } else {
            "\nNo VPS targets selected: SSH tools unavailable; use project_*, cloud_*, tfc_*, terraform_* for infra."
        });
    }
    estimate_tokens(&runtime)
}

fn segment(key: &str, label: &str, tokens: u32) -> ContextSegment {
    ContextSegment {
        key: key.into(),
        label: label.into(),
        tokens,
    }
}
