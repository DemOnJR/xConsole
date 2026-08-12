//! Estimated per-turn cost from provider usage, plus cache hit rate.
//!
//! Providers report token usage (input, output, cache read, cache write) but not
//! dollars. This module keeps a small price table for known providers/models so the
//! UI can show a live cost estimate and cache economics (pi-style footer: `$0.0123 ·
//! R 42K · W 3K · 93% hit`). Prices are per 1M tokens, USD, and can be overridden
//! by the user via settings (`agent.cost_input`, etc.).

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
const MODELS: &[(&str, &str, ModelPrice)] = &[
    // (kind, model-substring, price)
    ("anthropic", "opus", ModelPrice { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 }),
    ("anthropic", "sonnet", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }),
    ("anthropic", "haiku", ModelPrice { input: 0.80, output: 4.0, cache_read: 0.08, cache_write: 1.0 }),
    ("openai", "gpt-5", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-4", ModelPrice { input: 2.50, output: 10.0, cache_read: 0.25, cache_write: 3.125 }),
    ("openai", "o3", ModelPrice { input: 2.0, output: 8.0, cache_read: 0.20, cache_write: 2.50 }),
    ("deepseek", "deepseek", ModelPrice { input: 0.27, output: 1.10, cache_read: 0.027, cache_write: 0.34 }),
    // Fallback for anything else: a conservative mid-range price.
    ("", "", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.20, cache_write: 2.50 }),
];

/// Best-effort price for a provider kind + model name. Falls back to the last entry.
pub fn price_for(kind: &str, model: &str) -> ModelPrice {
    let k = kind.to_lowercase();
    let m = model.to_lowercase();
    for (kind_sub, model_sub, price) in MODELS {
        if !kind_sub.is_empty() && k.contains(kind_sub) {
            // Exact kind matches; prefer a model-substring match within it.
            if !model_sub.is_empty() && m.contains(model_sub) {
                return *price;
            }
        }
    }
    // Second pass: model-substring match regardless of kind (e.g. "claude-opus").
    for (_, model_sub, price) in MODELS {
        if !model_sub.is_empty() && m.contains(model_sub) {
            return *price;
        }
    }
    MODELS[MODELS.len() - 1].2
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
    let input_tokens = input.unwrap_or(0);
    let cache_read_tokens = cache_read.unwrap_or(0);
    let cache_write_tokens = cache_write.unwrap_or(0);
    let usd = input_tokens as f64 / 1_000_000.0 * price.input
        + output as f64 / 1_000_000.0 * price.output
        + cache_read_tokens as f64 / 1_000_000.0 * price.cache_read
        + cache_write_tokens as f64 / 1_000_000.0 * price.cache_write;
    TurnCost {
        input_tokens,
        output_tokens: output,
        cache_read_tokens,
        cache_write_tokens,
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
    fn turn_cost_uses_cache_prices() {
        let cost = turn_cost("anthropic", "claude-sonnet-4-6", Some(1000), 500, Some(40_000), Some(1000));
        // 1000 input @ $3/M + 500 out @ $15/M + 40K read @ $0.30/M + 1K write @ $3.75/M
        let expected = 0.003 + 0.0075 + 0.012 + 0.00375;
        assert!((cost.usd - expected).abs() < 1e-9);
        assert_eq!(cost.cache_read_tokens, 40_000);
        assert_eq!(cost.input_tokens, 1000);
    }

    #[test]
    fn cache_reads_are_cheaper_than_fresh_input() {
        // Reading 1M cached tokens should cost less than paying full input price.
        let price = price_for("anthropic", "claude-sonnet-4-6");
        let fresh = turn_cost("anthropic", "claude-sonnet-4-6", Some(1_000_000), 0, None, None);
        let cached = turn_cost("anthropic", "claude-sonnet-4-6", None, 0, Some(1_000_000), None);
        assert!(cached.usd < fresh.usd);
        assert!(cached.usd < price.input); // reads are a fraction of input
    }
}
