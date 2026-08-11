//! Short-TTL cache for read-only tool results and web_search.
//! Cuts repeat SSH inspections and identical search queries within a session.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Entry {
    value: String,
    expires: Instant,
}

static CACHE: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();

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
