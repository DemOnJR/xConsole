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

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Price per 1M tokens in USD for one model.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Which cache TTL a request asked the provider for.
///
/// Anthropic prices a cache **write** at 1.25x the base input rate for the
/// 5-minute TTL and 2.0x for the 1-hour TTL; a read is 0.1x either way. The
/// price table below carries the 5-minute rate, so the 1-hour rate is
/// `cache_write * 1.6` (= 2.0 / 1.25). Billing a 1h write at the 5m rate
/// under-reports an autonomous loop's real spend by 60% of its write bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CacheTtl {
    #[default]
    FiveMinutes,
    OneHour,
}

impl CacheTtl {
    /// Parse the `agent.cache_retention` setting / `ChatRequest::cache_retention`.
    pub fn from_retention(retention: &str) -> Self {
        if retention.eq_ignore_ascii_case("long") {
            CacheTtl::OneHour
        } else {
            CacheTtl::FiveMinutes
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CacheTtl::FiveMinutes => "5m",
            CacheTtl::OneHour => "1h",
        }
    }
}

impl ModelPrice {
    /// Write price per 1M tokens for a given TTL.
    ///
    /// A new field on `ModelPrice` would break every struct literal that builds
    /// one (including the user-facing price override command), so the 1-hour
    /// rate is derived from the 5-minute rate instead: 2.0x input / 1.25x input.
    pub fn cache_write_for(&self, ttl: CacheTtl) -> f64 {
        match ttl {
            CacheTtl::FiveMinutes => self.cache_write,
            CacheTtl::OneHour => self.cache_write * 1.6,
        }
    }
}

/// Dynamic price overrides synced from live endpoints or set by user.
static DYNAMIC_PRICES: LazyLock<RwLock<HashMap<String, ModelPrice>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Known models. `kind` matches the provider kind (anthropic, openai, ...) so a
/// default can be picked when the exact model name is unknown.
///
/// More-specific model substrings must come first: `v4-flash` before `deepseek`.
const MODELS: &[(&str, &str, ModelPrice)] = &[
    // (kind, model-substring, price in USD per 1M tokens)
    // --- Anthropic & Claude ---
    // Opus tier is $5 / $25 per MTok, NOT $15 / $75 — the old numbers were a
    // 3x over-estimate that made every cache decision look cheaper than it was.
    // `cache_write` here is always the 5-minute rate (1.25x input); the 1-hour
    // rate is 2.0x input and is derived by `ModelPrice::cache_write_for`.
    ("anthropic", "claude-fable-5-1", ModelPrice { input: 10.0, output: 50.0, cache_read: 0.25, cache_write: 12.5 }),
    ("anthropic", "claude-fable-5", ModelPrice { input: 10.0, output: 50.0, cache_read: 1.0, cache_write: 12.5 }),
    ("anthropic", "claude-opus-5", ModelPrice { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }),
    ("anthropic", "claude-opus-4-8", ModelPrice { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }),
    ("anthropic", "claude-opus-4-7", ModelPrice { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }),
    ("anthropic", "claude-opus-4-6", ModelPrice { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }),
    ("anthropic", "opus", ModelPrice { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }),
    ("anthropic", "claude-sonnet-5", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.20, cache_write: 2.50 }),
    ("anthropic", "claude-sonnet-4-6", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }),
    ("anthropic", "claude-3-7-sonnet", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }),
    ("anthropic", "claude-3-5-sonnet", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }),
    ("anthropic", "sonnet", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }),
    ("anthropic", "claude-haiku-4-5", ModelPrice { input: 1.0, output: 5.0, cache_read: 0.10, cache_write: 1.25 }),
    ("anthropic", "claude-3-5-haiku", ModelPrice { input: 0.80, output: 4.0, cache_read: 0.08, cache_write: 1.0 }),
    ("anthropic", "haiku", ModelPrice { input: 0.80, output: 4.0, cache_read: 0.08, cache_write: 1.0 }),

    // --- OpenAI ---
    ("openai", "gpt-5.6", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-5.5", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-5.4", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-5.3", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-5-mini", ModelPrice { input: 0.25, output: 1.25, cache_read: 0.025, cache_write: 0.3125 }),
    ("openai", "gpt-5", ModelPrice { input: 1.25, output: 10.0, cache_read: 0.125, cache_write: 1.5625 }),
    ("openai", "gpt-4.5", ModelPrice { input: 75.0, output: 150.0, cache_read: 37.5, cache_write: 75.0 }),
    ("openai", "gpt-4o-mini", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.075, cache_write: 0.15 }),
    ("openai", "gpt-4o", ModelPrice { input: 2.50, output: 10.0, cache_read: 1.25, cache_write: 2.50 }),
    ("openai", "gpt-4", ModelPrice { input: 2.50, output: 10.0, cache_read: 0.25, cache_write: 3.125 }),
    ("openai", "o3-mini", ModelPrice { input: 1.10, output: 4.40, cache_read: 0.55, cache_write: 1.10 }),
    ("openai", "o3", ModelPrice { input: 2.0, output: 8.0, cache_read: 0.20, cache_write: 2.50 }),
    ("openai", "o1-mini", ModelPrice { input: 1.10, output: 4.40, cache_read: 0.55, cache_write: 1.10 }),
    ("openai", "o1", ModelPrice { input: 15.0, output: 60.0, cache_read: 7.50, cache_write: 15.0 }),

    // --- DeepSeek ---
    ("deepseek", "v4-flash", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.0028, cache_write: 0.14 }),
    ("deepseek", "v4-pro", ModelPrice { input: 0.435, output: 0.87, cache_read: 0.003625, cache_write: 0.435 }),
    ("deepseek", "deepseek-reasoner", ModelPrice { input: 0.55, output: 2.19, cache_read: 0.14, cache_write: 0.55 }),
    ("deepseek", "deepseek-r1", ModelPrice { input: 0.55, output: 2.19, cache_read: 0.14, cache_write: 0.55 }),
    ("deepseek", "deepseek-chat", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.014, cache_write: 0.14 }),
    ("deepseek", "deepseek-v3", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.014, cache_write: 0.14 }),
    ("deepseek", "flash", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.0028, cache_write: 0.14 }),
    ("deepseek", "deepseek", ModelPrice { input: 0.14, output: 0.28, cache_read: 0.0028, cache_write: 0.14 }),

    // --- Ox Alpha (Stealth Reasoning) ---
    ("openai", "ox-alpha", ModelPrice { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 }),
    ("openai", "0x-alpha", ModelPrice { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 }),
    ("openai", "stealth", ModelPrice { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 }),

    // --- Google Gemini & Antigravity CLI ---
    ("gemini", "gemini-3.7-flash", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("gemini", "gemini-3.6-flash", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("gemini", "gemini-3.5-flash", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("gemini", "gemini-3.1-pro", ModelPrice { input: 1.25, output: 5.0, cache_read: 0.3125, cache_write: 1.25 }),
    ("gemini", "gemini-2.5-flash", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("gemini", "gemini-2.5-pro", ModelPrice { input: 1.25, output: 5.0, cache_read: 0.3125, cache_write: 1.25 }),
    ("gemini", "flash", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("gemini", "pro", ModelPrice { input: 1.25, output: 5.0, cache_read: 0.3125, cache_write: 1.25 }),
    ("gemini", "", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("antigravity_cli", "flash", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),
    ("antigravity_cli", "pro", ModelPrice { input: 1.25, output: 5.0, cache_read: 0.3125, cache_write: 1.25 }),
    ("antigravity_cli", "", ModelPrice { input: 0.15, output: 0.60, cache_read: 0.0375, cache_write: 0.15 }),

    // --- xAI Grok ---
    ("xai", "grok-4.6", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.50, cache_write: 2.0 }),
    ("xai", "grok-4.5", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.50, cache_write: 2.0 }),
    ("xai", "grok-3", ModelPrice { input: 3.0, output: 15.0, cache_read: 0.75, cache_write: 3.0 }),
    ("xai", "grok", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.50, cache_write: 2.0 }),

    // --- Qwen / Alibaba ---
    ("qwen", "qwen3.8-max", ModelPrice { input: 0.40, output: 1.20, cache_read: 0.10, cache_write: 0.40 }),
    ("qwen", "qwen3.7-plus", ModelPrice { input: 0.20, output: 0.60, cache_read: 0.05, cache_write: 0.20 }),
    ("qwen", "qwen3-max", ModelPrice { input: 0.40, output: 1.20, cache_read: 0.10, cache_write: 0.40 }),
    ("qwen", "qwen3-flash", ModelPrice { input: 0.10, output: 0.30, cache_read: 0.02, cache_write: 0.10 }),
    ("qwen", "qwen-max", ModelPrice { input: 0.40, output: 1.20, cache_read: 0.10, cache_write: 0.40 }),
    ("qwen", "qwen-plus", ModelPrice { input: 0.20, output: 0.60, cache_read: 0.05, cache_write: 0.20 }),
    ("qwen", "qwen-turbo", ModelPrice { input: 0.05, output: 0.20, cache_read: 0.01, cache_write: 0.05 }),

    // --- Moonshot / Kimi ---
    ("moonshot", "kimi-k3", ModelPrice { input: 0.40, output: 1.60, cache_read: 0.10, cache_write: 0.40 }),
    ("moonshot", "kimi-k2.7-code", ModelPrice { input: 0.30, output: 1.20, cache_read: 0.08, cache_write: 0.30 }),
    ("moonshot", "kimi-k2", ModelPrice { input: 0.30, output: 1.20, cache_read: 0.08, cache_write: 0.30 }),
    ("moonshot", "kimi", ModelPrice { input: 0.40, output: 1.60, cache_read: 0.10, cache_write: 0.40 }),
    ("moonshot", "", ModelPrice { input: 0.40, output: 1.60, cache_read: 0.10, cache_write: 0.40 }),

    // --- GLM & MiniMax ---
    ("glm", "glm-5.3", ModelPrice { input: 0.50, output: 1.50, cache_read: 0.10, cache_write: 0.50 }),
    ("minimax", "minimax-m3", ModelPrice { input: 0.20, output: 0.80, cache_read: 0.05, cache_write: 0.20 }),
    ("minimax", "minimax-m2.5", ModelPrice { input: 0.20, output: 0.80, cache_read: 0.05, cache_write: 0.20 }),

    // --- Mistral ---
    ("mistral", "mistral-large", ModelPrice { input: 2.0, output: 6.0, cache_read: 0.50, cache_write: 2.0 }),
    ("mistral", "codestral", ModelPrice { input: 0.30, output: 0.90, cache_read: 0.075, cache_write: 0.30 }),
    ("mistral", "mistral-small", ModelPrice { input: 0.20, output: 0.60, cache_read: 0.05, cache_write: 0.20 }),

    // Fallback for anything else: a conservative mid-range price.
    ("", "", ModelPrice { input: 2.0, output: 10.0, cache_read: 0.20, cache_write: 2.50 }),
];

/// Register or override a dynamic price for a specific model id.
pub fn register_dynamic_price(model_id: &str, price: ModelPrice) {
    if let Ok(mut map) = DYNAMIC_PRICES.write() {
        map.insert(model_id.to_lowercase(), price);
    }
}

/// Retrieve all known prices (combining static and dynamic entries).
pub fn get_all_model_prices() -> HashMap<String, ModelPrice> {
    let mut out = HashMap::new();
    for (_, model_sub, price) in MODELS {
        if !model_sub.is_empty() {
            out.insert(model_sub.to_string(), *price);
        }
    }
    if let Ok(map) = DYNAMIC_PRICES.read() {
        for (k, v) in map.iter() {
            out.insert(k.clone(), *v);
        }
    }
    out
}

/// Automatically sync model prices from OpenRouter's live public catalog.
pub async fn sync_online_prices() -> Result<usize, String> {
    let url = "https://openrouter.ai/api/v1/models";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GET {url} -> {}", resp.status()));
    }

    let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let data = val.get("data").and_then(|d| d.as_array()).ok_or_else(|| "missing data array in models response".to_string())?;

    let mut count = 0;
    for item in data {
        if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
            if let Some(pricing) = item.get("pricing") {
                let parse_num = |field: &str| -> f64 {
                    if let Some(v) = pricing.get(field) {
                        if let Some(n) = v.as_f64() {
                            return n * 1_000_000.0;
                        }
                        if let Some(s) = v.as_str() {
                            if let Ok(n) = s.parse::<f64>() {
                                return n * 1_000_000.0;
                            }
                        }
                    }
                    0.0
                };
                let input = parse_num("prompt");
                let output = parse_num("completion");
                let cache_read = {
                    let r = parse_num("input_cache_read");
                    if r > 0.0 { r } else { input * 0.1 }
                };
                let cache_write = input;

                if input > 0.0 || output > 0.0 {
                    register_dynamic_price(id, ModelPrice {
                        input,
                        output,
                        cache_read,
                        cache_write,
                    });
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

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
    if m.contains("gemini") {
        return "gemini".into();
    }
    if m.contains("grok") {
        return "xai".into();
    }
    if m.contains("qwen") {
        return "qwen".into();
    }
    if m.contains("kimi") || m.contains("moonshot") {
        return "moonshot".into();
    }
    if m.contains("minimax") {
        return "minimax".into();
    }
    if m.contains("glm") {
        return "glm".into();
    }
    if m.contains("mistral") || m.contains("codestral") {
        return "mistral".into();
    }
    kind.to_lowercase()
}

/// Best-effort price for a provider kind + model name. Falls back to the last entry.
pub fn price_for(kind: &str, model: &str) -> ModelPrice {
    let m = model.to_lowercase();

    // Check dynamic prices first
    if let Ok(map) = DYNAMIC_PRICES.read() {
        if let Some(p) = map.get(&m) {
            return *p;
        }
        for (dyn_k, dyn_v) in map.iter() {
            if m.contains(dyn_k) || dyn_k.contains(&m) {
                return *dyn_v;
            }
        }
    }

    let k = kind_for_model(kind, model);
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

/// Cache hit rate 0..1 from provider-reported prompt + cached + written counts.
///
/// **Written tokens belong in the denominator.** A request that rewrites the
/// whole prefix reports `input_tokens: 0`, `cache_read_input_tokens: 0` and a
/// large `cache_creation_input_tokens` - with writes excluded that scored as
/// "0 hit · 0 miss · 0%" and was then filtered out by the miss-reason guards,
/// so the single most expensive failure mode was invisible. A write is a miss
/// you also paid a premium on; count it as one.
pub fn cache_hit_rate(prompt: u32, cached: u32, written: u32) -> f64 {
    let split = split_usage(prompt, cached, written, 0);
    let denom = split
        .fresh_input
        .saturating_add(split.cached)
        .saturating_add(split.written);
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
    /// Full-price uncached input.
    pub miss: u32,
    /// Tokens written to the cache this request (billed at the write premium).
    pub written: u32,
    /// `hit / (hit + miss + written)`.
    pub rate: f64,
}

impl CacheReport {
    /// Every prompt token this request was billed for, however it was billed.
    pub fn total(&self) -> u32 {
        self.hit
            .saturating_add(self.miss)
            .saturating_add(self.written)
    }
}

pub fn cache_report(prompt: u32, cached: u32, written: u32) -> CacheReport {
    let split = split_usage(prompt, cached, written, 0);
    CacheReport {
        hit: split.cached,
        miss: split.fresh_input,
        written: split.written,
        rate: cache_hit_rate(prompt, cached, written),
    }
}

/// Compact line for the agent transcript:
/// `cache 1664 hit · 68 miss · 512 written · 96%`.
pub fn format_cache_line(prompt: u32, cached: u32, written: u32) -> String {
    let r = cache_report(prompt, cached, written);
    format!(
        "cache {} hit · {} miss · {} written · {:.0}%",
        r.hit,
        r.miss,
        r.written,
        r.rate * 100.0
    )
}

/// Token-weighted cache hit rate across many requests.
///
/// Averaging per-request percentages treats a 40-token request and a 400K-token
/// request as equals, so one cheap high-hit turn can hide an expensive full
/// rewrite. Weight by tokens: `sum(hit) / sum(hit + miss + written)`.
pub fn weighted_cache_hit_rate<I>(rows: I) -> f64
where
    I: IntoIterator<Item = (u32, u32, u32)>,
{
    let mut hit: u64 = 0;
    let mut total: u64 = 0;
    for (h, m, w) in rows {
        hit += h as u64;
        total += h as u64 + m as u64 + w as u64;
    }
    if total == 0 {
        0.0
    } else {
        hit as f64 / total as f64
    }
}

/// Extra diagnostic when a miss is *not* a cold start or 128-token alignment remainder.
/// `classification` is the prefix-telemetry label (`first_request`, `append_only`, …).
/// `request_index` is 0 for the first model call of this user turn.
pub fn cache_miss_reason(
    prompt: u32,
    cached: u32,
    written: u32,
    classification: &str,
    request_index: u32,
) -> Option<String> {
    let r = cache_report(prompt, cached, written);
    if r.total() == 0 {
        return None;
    }
    // First call of a brand-new conversation: only warn when it is actually cold.
    // A high hit on first_request is leftover prefix cache (same system/tools) —
    // not a miss worth logging. A cold start legitimately writes the whole
    // prefix, so it is checked before the rewrite alarm below.
    if classification == "first_request" && request_index == 0 {
        if r.rate < 0.5 {
            return Some(format!(
                "cache cold start — first request writes the prefix ({} miss, {} written)",
                r.miss, r.written
            ));
        }
        return None;
    }
    // A prefix rewrite is the expensive failure and must never be filtered out
    // by the healthy-growth guards below: writing more than we read means the
    // request paid the 1.25x/2.0x premium on bytes it could have read at 0.1x.
    // Without writes in the denominator this logged as "0 hit · 0 miss · 0%"
    // and the guards then dropped it entirely.
    if r.written > 0 && r.written > r.hit {
        return Some(format!(
            "cache REWRITE: {} written vs {} read · {:.0}% hit — the prefix changed \
             upstream of the breakpoints ({classification})",
            r.written,
            r.hit,
            r.rate * 100.0
        ));
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
        "cache miss: {} miss {} written · {:.0}% hit — {why}",
        r.miss,
        r.written,
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

/// Compute the cost of one turn from provider-reported usage (5-minute TTL).
pub fn turn_cost(
    kind: &str,
    model: &str,
    input: Option<u32>,
    output: u32,
    cache_read: Option<u32>,
    cache_write: Option<u32>,
) -> TurnCost {
    turn_cost_ttl(
        kind,
        model,
        input,
        output,
        cache_read,
        cache_write,
        CacheTtl::FiveMinutes,
    )
}

/// Compute the cost of one turn, billing cache writes at the requested TTL.
pub fn turn_cost_ttl(
    kind: &str,
    model: &str,
    input: Option<u32>,
    output: u32,
    cache_read: Option<u32>,
    cache_write: Option<u32>,
    ttl: CacheTtl,
) -> TurnCost {
    let price = price_for(kind, model);
    let prompt = input.unwrap_or(0);
    let cached = cache_read.unwrap_or(0);
    let written = cache_write.unwrap_or(0);
    let split = split_usage(prompt, cached, written, output);
    let usd = split.fresh_input as f64 / 1_000_000.0 * price.input
        + split.output as f64 / 1_000_000.0 * price.output
        + split.cached as f64 / 1_000_000.0 * price.cache_read
        + split.written as f64 / 1_000_000.0 * price.cache_write_for(ttl);
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
        assert_eq!(price_for("anthropic", "claude-opus-5").input, 5.0);
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
        assert!((cache_hit_rate(1_000, 39_000, 0) - 0.975).abs() < 1e-9);
        // DeepSeek inclusive: 50k prompt of which 48k cached.
        assert!((cache_hit_rate(50_000, 48_000, 0) - 0.96).abs() < 1e-9);
        assert_eq!(cache_hit_rate(0, 0, 0), 0.0);
    }

    #[test]
    fn cache_report_counts_writes() {
        // The failure this exists to catch: a request that rewrote the entire
        // 24K prefix. input=0, read=0, written=24_000 used to report
        // "0 hit · 0 miss · 0%" and was then dropped by the miss-reason guards.
        let r = cache_report(0, 0, 24_000);
        assert_eq!(r.written, 24_000);
        assert_eq!(r.total(), 24_000);
        assert_eq!(r.rate, 0.0);
        assert!(format_cache_line(0, 0, 24_000).contains("24000 written"));
        let why = cache_miss_reason(0, 0, 24_000, "append_only", 3).unwrap();
        assert!(why.contains("REWRITE"), "{why}");

        // A healthy append-only turn writes only the new tail.
        let healthy = cache_report(120, 38_000, 900);
        assert_eq!(healthy.written, 900);
        assert!((healthy.rate - 38_000.0 / 39_020.0).abs() < 1e-9);
        assert!(cache_miss_reason(120, 38_000, 900, "append_only", 2).is_none());
    }

    #[test]
    fn weighted_average_is_token_weighted_not_per_request() {
        // Two requests: a tiny 100% one and a huge full rewrite.
        let rows = vec![(100u32, 0u32, 0u32), (0u32, 0u32, 100_000u32)];
        let unweighted: f64 = (100.0 + 0.0) / 2.0 / 100.0; // the old analytics maths
        let weighted = weighted_cache_hit_rate(rows);
        assert!((unweighted - 0.5).abs() < 1e-9);
        assert!(weighted < 0.002, "weighted={weighted}");
        assert_eq!(weighted_cache_hit_rate(Vec::new()), 0.0);
    }

    #[test]
    fn one_hour_writes_cost_more_than_five_minute_writes() {
        let p = price_for("anthropic", "claude-opus-5");
        // 5m = 1.25x input, 1h = 2.0x input.
        assert!((p.cache_write - p.input * 1.25).abs() < 1e-9);
        assert!((p.cache_write_for(CacheTtl::OneHour) - p.input * 2.0).abs() < 1e-9);
        let short = turn_cost_ttl("anthropic", "claude-opus-5", Some(0), 0, None, Some(1_000_000), CacheTtl::FiveMinutes);
        let long = turn_cost_ttl("anthropic", "claude-opus-5", Some(0), 0, None, Some(1_000_000), CacheTtl::OneHour);
        assert!((short.usd - 6.25).abs() < 1e-9, "{short:?}");
        assert!((long.usd - 10.0).abs() < 1e-9, "{long:?}");
        assert_eq!(CacheTtl::from_retention("long"), CacheTtl::OneHour);
        assert_eq!(CacheTtl::from_retention(""), CacheTtl::FiveMinutes);
    }

    #[test]
    fn opus_tier_is_five_and_twentyfive_per_mtok() {
        for m in ["claude-opus-5", "claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6"] {
            let p = price_for("anthropic", m);
            assert_eq!((p.input, p.output), (5.0, 25.0), "{m}");
            assert_eq!(p.cache_read, 0.50, "{m}");
        }
        let s5 = price_for("anthropic", "claude-sonnet-5");
        assert_eq!((s5.input, s5.output), (2.0, 10.0));
    }

    #[test]
    fn cache_line_and_miss_reason() {
        assert_eq!(
            format_cache_line(50_000, 48_000, 0),
            "cache 48000 hit · 2000 miss · 0 written · 96%"
        );
        assert!(cache_miss_reason(50_000, 48_000, 0, "append_only", 1).is_none());
        let cold = cache_miss_reason(2000, 0, 0, "first_request", 0).unwrap();
        assert!(cold.contains("cold start"));
        let sys = cache_miss_reason(20_000, 1000, 0, "system", 1).unwrap();
        assert!(sys.contains("system prefix changed"));
        assert!(cache_miss_reason(1732, 1664, 0, "append_only", 2).is_none());
        // Installed-app turn 3: 80% / 2372 miss is healthy append-only growth.
        assert!(cache_miss_reason(11_844, 9_472, 0, "append_only", 0).is_none());
        // A real fat tail (20% hit) is still logged.
        let fat = cache_miss_reason(10_000, 2_000, 0, "append_only", 1).unwrap();
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

    #[test]
    fn dynamic_pricing_overrides_and_resolves_custom_models() {
        register_dynamic_price("custom/fast-model-v1", ModelPrice {
            input: 0.05,
            output: 0.15,
            cache_read: 0.005,
            cache_write: 0.05,
        });
        let p = price_for("openai", "custom/fast-model-v1");
        assert_eq!(p.input, 0.05);
        assert_eq!(p.output, 0.15);
        assert_eq!(p.cache_read, 0.005);
    }
}
