//! Estimated per-turn cost from provider usage, plus cache hit rate.
//!
//! Providers report token usage (input, output, cache read, cache write) but not
//! dollars. This module keeps a small price table for known providers/models so the
//! UI can show a live cost estimate and cache economics (cost footer: `$0.0123 ·
//! R 42K · W 3K · 93% hit`). Prices are per 1M tokens, USD, and can be overridden
//! by the user via settings (`agent.cost_input`, etc.).
//!
//! ## Inclusive vs exclusive prompt counts
//!
//! OpenAI, DeepSeek, and Command Code report `prompt_tokens` as the **total**
//! input, with cached tokens as a subset (`cached_tokens` /
//! `prompt_cache_hit_tokens`). Anthropic reports `input_tokens` as the cache-miss
//! count only, plus a separate `cache_read_input_tokens`. Charging
//! `prompt * input_price + cached * cache_read_price` double-counts the cached
//! tokens on inclusive providers and overstates DeepSeek bills by ~50×.

/// Price per 1M tokens in USD for one model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Known models. `kind` matches the provider kind (anthropic, openai, ...) so a
/// default can be picked when the exact model name is unknown.
///
/// More-specific model substrings must come first: `v4-flash` before `deepseek`.
const MODELS: &[(&str, &str, ModelPrice)] = &[
    // (kind, model-substring, price)
    ("anthropic", "opus", ModelPrice { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 }),
    ("anthropic", "sonnet", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }),
    ("anthropic", "haiku", ModelPrice { input: 0.80, output: 4.0, cache_read: 0.08, cache_write: 1.0 }),
    ("openai", "gpt-5", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-4", ModelPrice { input: 2.50, output: 10.0, cache_read: 0.25, cache_write: 3.125 }),
    ("openai", "o3", ModelPrice { input: 2.0, output: 8.0, cache_read: 0.20, cache_write: 2.50 }),
    // DeepSeek V4 Flash / Command Code Flash — official Aug 2026 list prices.
    // No separate cache-write line: first-seen tokens bill at miss (input) price.
    ("deepseek", "v4-flash", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.0028, cache_write: 0.14 }),
    ("deepseek", "flash", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.0028, cache_write: 0.14 }),
    ("deepseek", "v4-pro", ModelPrice { input: 0.435, output: 0.87, cache_read: 0.003625, cache_write: 0.435 }),
    ("deepseek", "deepseek", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.0028, cache_write: 0.14 }),
    // Fallback for anything else: a conservative mid-range price.
    ("", "", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.20, cache_write: 2.50 }),
];

/// Infer a pricing kind from the model id when the wire adapter is generic
/// OpenAI-compat (Command Code, OpenRouter, …).
pub fn kind_for_model(kind: &str, model: &str) -> String {
    let m = model.to_lowercase();
    if m.contains("deepseek") {
        return "deepseek".into();
    }
    if m.contains("claude") {
        return "anthropic".into();
    }
    kind.to_lowercase()
}

/// Best-effort price for a provider kind + model name. Falls back to the last entry.
pub fn price_for(kind: &str, model: &str) -> ModelPrice {
    let k = kind_for_model(kind, model);
    let m = model.to_lowercase();
    for (kind_sub, model_sub, price) in MODELS {
        if !kind_sub.is_empty() && k.contains(kind_sub) {
            if !model_sub.is_empty() && m.contains(model_sub) {
                return *price;
            }
        }
    }
    // Second pass: model-substring match regardless of kind (e.g. "claude-opus",
    // "deepseek/deepseek-v4-flash" billed through an openai-compat adapter).
    for (_, model_sub, price) in MODELS {
        if !model_sub.is_empty() && m.contains(model_sub) {
            return *price;
        }
    }
    MODELS[MODELS.len() - 1].2
}

/// Prompt tokens split into billable fresh vs cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitUsage {
    pub fresh_input: u32,
    pub cached: u32,
    pub written: u32,
    pub output: u32,
}

/// Split provider usage into fresh/cached without double-counting.
///
/// If `cached <= prompt` and `cached > 0`, treat `prompt` as **inclusive**
/// (OpenAI / DeepSeek / Command Code). If `cached > prompt`, treat `prompt` as
/// **exclusive** miss tokens (Anthropic).
pub fn split_usage(prompt: u32, cached: u32, written: u32, output: u32) -> SplitUsage {
    if cached > 0 && cached <= prompt {
        SplitUsage {
            fresh_input: prompt - cached,
            cached,
            written,
            output,
        }
    } else {
        SplitUsage {
            fresh_input: prompt,
            cached,
            written,
            output,
        }
    }
}

/// Expected prefix-cache hit rate for an append-only request.
///
/// DeepSeek (and most gateways) cache in 128-token blocks, so the last
/// incomplete block of the prefix is billed as a miss even when the bytes
/// match. `tail` is the new last-user / dynamic tokens that cannot hit.
pub fn expected_prefix_hit_rate(prefix_tokens: u32, tail_tokens: u32) -> f64 {
    const BLOCK: u32 = 128;
    let cached = (prefix_tokens / BLOCK) * BLOCK;
    let total = prefix_tokens.saturating_add(tail_tokens);
    if total == 0 {
        0.0
    } else {
        cached as f64 / total as f64
    }
}

/// Cache hit rate 0..1 from provider-reported prompt + cached counts.
pub fn cache_hit_rate(prompt: u32, cached: u32) -> f64 {
    let split = split_usage(prompt, cached, 0, 0);
    let denom = split.fresh_input.saturating_add(split.cached);
    if denom == 0 {
        0.0
    } else {
        split.cached as f64 / denom as f64
    }
}

/// One request's cache accounting, ready to show in the agent terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CacheReport {
    pub hit: u32,
    pub miss: u32,
    pub rate: f64,
}

pub fn cache_report(prompt: u32, cached: u32) -> CacheReport {
    let split = split_usage(prompt, cached, 0, 0);
    CacheReport {
        hit: split.cached,
        miss: split.fresh_input,
        rate: cache_hit_rate(prompt, cached),
    }
}

/// Compact line for the agent transcript: `cache 1664 hit · 68 miss · 96%`.
pub fn format_cache_line(prompt: u32, cached: u32) -> String {
    let r = cache_report(prompt, cached);
    format!(
        "cache {} hit · {} miss · {:.0}%",
        r.hit,
        r.miss,
        r.rate * 100.0
    )
}

/// Extra diagnostic when a miss is *not* a cold start or 128-token alignment remainder.
/// `classification` is the prefix-telemetry label (`first_request`, `append_only`, …).
/// `request_index` is 0 for the first model call of this user turn.
pub fn cache_miss_reason(
    prompt: u32,
    cached: u32,
    classification: &str,
    request_index: u32,
) -> Option<String> {
    let r = cache_report(prompt, cached);
    if r.hit + r.miss == 0 {
        return None;
    }
    // First call of a brand-new conversation: only warn when it is actually cold.
    // A high hit on first_request is leftover prefix cache (same system/tools) —
    // not a miss worth logging.
    if classification == "first_request" && request_index == 0 {
        if r.rate < 0.5 {
            return Some(format!(
                "cache cold start — first request writes the prefix ({} miss)",
                r.miss
            ));
        }
        return None;
    }
    // DeepSeek/OpenAI 128-token block remainder is not a real prefix break.
    if r.miss <= 128 && r.rate >= 0.85 {
        return None;
    }
    if r.rate >= 0.95 {
        return None;
    }
    // Append-only growth: miss is previous assistant + new user + this turn's
    // runtime. Short sessions land at 80–90% and that is healthy — the installed
    // "hi / how are you" chat logged 80% as a false alarm. Only warn when the
    // tail is still eating most of the prompt.
    if classification == "append_only" && (r.rate >= 0.70 || (r.miss <= 4_096 && r.rate >= 0.50))
    {
        return None;
    }
    let why = match classification {
        "system" => "system prefix changed (soul / taste / skills / tools / safety)",
        "schema" => "tool schema changed",
        "message_prefix" => "history rewritten (compaction or window)",
        "first_request" => "new turn started a fresh prefix fingerprint",
        "append_only" => "large uncached tail (canvas / memory / new user text)",
        other => other,
    };
    Some(format!(
        "cache miss: {} miss · {:.0}% hit — {why}",
        r.miss,
        r.rate * 100.0
    ))
}

/// A single turn's cost accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct TurnCost {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    /// Estimated USD for this turn.
    pub usd: f64,
}

/// Compute the cost of one turn from provider-reported usage.
pub fn turn_cost(
    kind: &str,
    model: &str,
    input: Option<u32>,
    output: u32,
    cache_read: Option<u32>,
    cache_write: Option<u32>,
) -> TurnCost {
    let price = price_for(kind, model);
    let prompt = input.unwrap_or(0);
    let cached = cache_read.unwrap_or(0);
    let written = cache_write.unwrap_or(0);
    let split = split_usage(prompt, cached, written, output);
    let usd = split.fresh_input as f64 / 1_000_000.0 * price.input
        + split.output as f64 / 1_000_000.0 * price.output
        + split.cached as f64 / 1_000_000.0 * price.cache_read
        + split.written as f64 / 1_000_000.0 * price.cache_write;
    TurnCost {
        input_tokens: split.fresh_input,
        output_tokens: output,
        cache_read_tokens: split.cached,
        cache_write_tokens: split.written,
        usd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_for_matches_model_substrings() {
        assert_eq!(price_for("anthropic", "claude-sonnet-4-6").input, 3.0);
        assert_eq!(price_for("anthropic", "claude-opus-5").input, 15.0);
        assert_eq!(price_for("openai", "gpt-5.6-luna").input, 1.25);
        // Unknown model falls back to the conservative default.
        assert_eq!(price_for("anthropic", "weird-model").input, 2.0);
    }

    #[test]
    fn deepseek_v4_flash_uses_official_2026_prices() {
        let p = price_for("openai", "deepseek/deepseek-v4-flash");
        assert_eq!(p.input, 0.14);
        assert_eq!(p.output, 0.28);
        assert_eq!(p.cache_read, 0.0028);
        let pro = price_for("deepseek", "deepseek-v4-pro");
        assert_eq!(pro.input, 0.435);
        assert_eq!(pro.cache_read, 0.003625);
    }

    #[test]
    fn turn_cost_uses_cache_prices() {
        let cost = turn_cost("anthropic", "claude-sonnet-4-6", Some(1000), 500, Some(40_000), Some(1000));
        // Anthropic: input is exclusive (1000 < 40k cached).
        // 1000 input @ $3/M + 500 out @ $15/M + 40K read @ $0.30/M + 1K write @ $3.75/M
        let expected = 0.003 + 0.0075 + 0.012 + 0.00375;
        assert!((cost.usd - expected).abs() < 1e-9);
        assert_eq!(cost.cache_read_tokens, 40_000);
        assert_eq!(cost.input_tokens, 1000);
    }

    #[test]
    fn inclusive_prompt_tokens_are_not_double_counted() {
        // DeepSeek/Command Code: prompt=50k includes 48k cached.
        let cost = turn_cost(
            "openai",
            "deepseek/deepseek-v4-flash",
            Some(50_000),
            200,
            Some(48_000),
            None,
        );
        let fresh = 2_000.0 / 1_000_000.0 * 0.14;
        let cached = 48_000.0 / 1_000_000.0 * 0.0028;
        let out = 200.0 / 1_000_000.0 * 0.28;
        assert!((cost.usd - (fresh + cached + out)).abs() < 1e-12);
        assert_eq!(cost.input_tokens, 2_000);
        assert_eq!(cost.cache_read_tokens, 48_000);
        // The old (buggy) formula billed all 50k at miss + 48k at hit.
        let double_counted = 50_000.0 / 1_000_000.0 * 0.14 + cached + out;
        assert!(cost.usd < double_counted);
    }

    #[test]
    fn cache_hit_rate_handles_both_reporting_styles() {
        // Anthropic exclusive: 1k miss + 39k hit.
        assert!((cache_hit_rate(1_000, 39_000) - 0.975).abs() < 1e-9);
        // DeepSeek inclusive: 50k prompt of which 48k cached.
        assert!((cache_hit_rate(50_000, 48_000) - 0.96).abs() < 1e-9);
        assert_eq!(cache_hit_rate(0, 0), 0.0);
    }

    #[test]
    fn cache_line_and_miss_reason() {
        assert_eq!(format_cache_line(50_000, 48_000), "cache 48000 hit · 2000 miss · 96%");
        assert!(cache_miss_reason(50_000, 48_000, "append_only", 1).is_none());
        let cold = cache_miss_reason(2000, 0, "first_request", 0).unwrap();
        assert!(cold.contains("cold start"));
        let sys = cache_miss_reason(20_000, 1000, "system", 1).unwrap();
        assert!(sys.contains("system prefix changed"));
        assert!(cache_miss_reason(1732, 1664, "append_only", 2).is_none());
        // Installed-app turn 3: 80% / 2372 miss is healthy append-only growth.
        assert!(cache_miss_reason(11_844, 9_472, "append_only", 0).is_none());
        // A real fat tail (20% hit) is still logged.
        let fat = cache_miss_reason(10_000, 2_000, "append_only", 1).unwrap();
        assert!(fat.contains("uncached tail"));
    }

    #[test]
    fn long_session_expected_hit_rate_reaches_95_percent() {
        // Short probe (589 tokens, no tail): 512/589 ≈ 86.9% — alignment, not a bug.
        assert!((expected_prefix_hit_rate(589, 0) - 512.0 / 589.0).abs() < 1e-9);
        // Realistic agent system (~4K) + tiny new user: already ≥95%.
        assert!(expected_prefix_hit_rate(4_000, 80) > 0.95);
        // Long session: 20K cached history + ~1K live tail (canvas/memory/date).
        assert!(expected_prefix_hit_rate(20_000, 1_000) > 0.95);
        // Fat tail (old 8K dynamic) keeps even a 40K session under 90%.
        assert!(expected_prefix_hit_rate(40_000, 8_000) < 0.90);
    }

    #[test]
    fn cache_reads_are_cheaper_than_fresh_input() {
        let price = price_for("anthropic", "claude-sonnet-4-6");
        let fresh = turn_cost("anthropic", "claude-sonnet-4-6", Some(1_000_000), 0, None, None);
        let cached = turn_cost("anthropic", "claude-sonnet-4-6", None, 0, Some(1_000_000), None);
        assert!(cached.usd < fresh.usd);
        assert!(cached.usd < price.input); // reads are a fraction of input
    }
}
