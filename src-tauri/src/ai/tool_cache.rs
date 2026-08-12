//! Short-TTL cache for read-only tool results and web_search.
//! Cuts repeat SSH inspections and identical search queries within a session.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Entry {
    value: String,
    expires: Instant,
}

static CACHE: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();

#[derive(Debug, Default, Clone)]
pub struct TurnTelemetry {
    pub tool_calls: Arc<AtomicU64>,
    pub tool_cache_lookups: Arc<AtomicU64>,
    pub tool_cache_hits: Arc<AtomicU64>,
    pub tool_cache_misses: Arc<AtomicU64>,
    pub tool_cache_writes: Arc<AtomicU64>,
}

impl TurnTelemetry {
    pub fn snapshot(&self) -> TurnTelemetrySnapshot {
        let lookups = self.tool_cache_lookups.load(Ordering::Relaxed);
        let hits = self.tool_cache_hits.load(Ordering::Relaxed);
        TurnTelemetrySnapshot {
            tool_calls: self.tool_calls.load(Ordering::Relaxed),
            tool_cache_lookups: lookups,
            tool_cache_hits: hits,
            tool_cache_misses: self.tool_cache_misses.load(Ordering::Relaxed),
            tool_cache_writes: self.tool_cache_writes.load(Ordering::Relaxed),
            tool_cache_hit_rate: if lookups == 0 { 0.0 } else { hits as f32 / lookups as f32 },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnTelemetrySnapshot {
    pub tool_calls: u64,
    pub tool_cache_lookups: u64,
    pub tool_cache_hits: u64,
    pub tool_cache_misses: u64,
    pub tool_cache_writes: u64,
    pub tool_cache_hit_rate: f32,
}

pub type TurnTelemetryHandle = Arc<TurnTelemetry>;

pub fn new_turn_telemetry() -> TurnTelemetryHandle {
    Arc::new(TurnTelemetry::default())
}

pub fn record_tool_call(telemetry: &TurnTelemetryHandle) {
    telemetry.tool_calls.fetch_add(1, Ordering::Relaxed);
}

pub fn record_cache_lookup(telemetry: &TurnTelemetryHandle, hit: bool) {
    telemetry.tool_cache_lookups.fetch_add(1, Ordering::Relaxed);
    let counter = if hit { &telemetry.tool_cache_hits } else { &telemetry.tool_cache_misses };
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn record_cache_write(telemetry: &TurnTelemetryHandle) {
    telemetry.tool_cache_writes.fetch_add(1, Ordering::Relaxed);
}

fn map() -> &'static Mutex<HashMap<u64, Entry>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Tools safe to cache (read-only / pure lookups).
pub fn is_cacheable(tool: &str) -> bool {
    matches!(
        tool,
        "list_vps_targets"
            | "read_file"
            | "local_read_file"
            | "local_list_dir"
            | "ssh_key_status"
            | "web_search"
            | "web_fetch"
            | "geo_locate"
            | "skills_list"
            | "skill_view"
            | "list_official_skills"
            | "host_memory_get"
    )
}

fn ttl_for(tool: &str) -> Duration {
    match tool {
        "web_search" | "web_fetch" | "geo_locate" => Duration::from_secs(120),
        "list_vps_targets" | "skills_list" | "list_official_skills" => Duration::from_secs(60),
        _ => Duration::from_secs(45),
    }
}

fn key(tool: &str, args: &Value) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tool.hash(&mut h);
    // Stable JSON for the args object.
    let s = serde_json::to_string(args).unwrap_or_default();
    s.hash(&mut h);
    h.finish()
}

pub fn get(tool: &str, args: &Value) -> Option<String> {
    if !is_cacheable(tool) {
        return None;
    }
    let k = key(tool, args);
    let mut guard = map().lock().ok()?;
    let now = Instant::now();
    // Opportunistic GC of a few expired entries.
    if guard.len() > 256 {
        guard.retain(|_, e| e.expires > now);
    }
    let e = guard.get(&k)?;
    if e.expires <= now {
        guard.remove(&k);
        return None;
    }
    Some(e.value.clone())
}

pub fn put(tool: &str, args: &Value, value: &str) {
    if !is_cacheable(tool) {
        return;
    }
    // Don't cache errors — agent should retry.
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("error:") || trimmed.starts_with("Error") {
        return;
    }
    let k = key(tool, args);
    if let Ok(mut guard) = map().lock() {
        guard.insert(
            k,
            Entry {
                value: value.to_string(),
                expires: Instant::now() + ttl_for(tool),
            },
        );
    }
}

/// Drop all entries (e.g. after major config change). Rarely needed.
#[allow(dead_code)]
pub fn clear() {
    if let Ok(mut guard) = map().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_tracks_cacheable_calls_and_hit_rate() {
        let telemetry = new_turn_telemetry();
        record_tool_call(&telemetry);
        record_tool_call(&telemetry);
        record_cache_lookup(&telemetry, true);
        record_cache_lookup(&telemetry, false);
        record_cache_write(&telemetry);
        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.tool_calls, 2);
        assert_eq!(snapshot.tool_cache_lookups, 2);
        assert_eq!(snapshot.tool_cache_hits, 1);
        assert_eq!(snapshot.tool_cache_misses, 1);
        assert_eq!(snapshot.tool_cache_writes, 1);
        assert!((snapshot.tool_cache_hit_rate - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn telemetry_zero_lookups_has_zero_rate() {
        let snapshot = new_turn_telemetry().snapshot();
        assert_eq!(snapshot.tool_cache_lookups, 0);
        assert_eq!(snapshot.tool_cache_hit_rate, 0.0);
    }

    #[test]
    fn cache_isolated_by_tool_and_arguments() {
        clear();
        let first = serde_json::json!({ "path": "a" });
        let second = serde_json::json!({ "path": "b" });
        put("local_read_file", &first, "a-result");
        assert_eq!(get("local_read_file", &first).as_deref(), Some("a-result"));
        assert_eq!(get("local_read_file", &second), None);
        assert_eq!(get("web_search", &first), None);
        clear();
    }

    #[test]
    fn invalid_and_non_cacheable_values_are_not_stored() {
        clear();
        let args = serde_json::json!({ "x": 1 });
        put("local_read_file", &args, "");
        assert_eq!(get("local_read_file", &args), None);
        put("local_read_file", &args, "error: failed");
        assert_eq!(get("local_read_file", &args), None);
        put("local_write_file", &args, "not cached");
        assert_eq!(get("local_write_file", &args), None);
        clear();
    }

    #[test]
    fn clear_removes_cached_entries() {
        clear();
        let args = serde_json::json!({ "path": "reset" });
        put("local_read_file", &args, "value");
        assert!(get("local_read_file", &args).is_some());
        clear();
        assert_eq!(get("local_read_file", &args), None);
    }
}
