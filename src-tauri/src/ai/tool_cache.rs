//! Short-TTL cache for read-only tool results and web_search.
//! Cuts repeat SSH inspections and identical search queries within a session.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Entry {
    scope: CacheScope,
    tool: String,
    args: Value,
    value: String,
    expires: Instant,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct CacheScope {
    pub session_id: String,
    pub workspace_id: String,
    pub targets: Vec<String>,
    pub home: String,
}

impl CacheScope {
    pub fn new(session_id: &str, workspace_id: Option<&str>, targets: &[String], home: &Path) -> Self {
        let mut targets = targets.to_vec();
        targets.sort();
        targets.dedup();
        Self {
            session_id: session_id.to_string(),
            workspace_id: workspace_id.unwrap_or_default().to_string(),
            targets,
            home: home.to_string_lossy().into_owned(),
        }
    }

    pub fn global() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalidation {
    RemoteFile { vps_id: String, path: String },
    LocalFile { path: String },
    HostMemory { vps_id: String },
    Skills,
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

fn key(scope: &CacheScope, tool: &str, args: &Value) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    scope.hash(&mut h);
    tool.hash(&mut h);
    let s = serde_json::to_string(args).unwrap_or_default();
    s.hash(&mut h);
    h.finish()
}

pub fn get(tool: &str, args: &Value) -> Option<String> {
    get_scoped(&CacheScope::global(), tool, args)
}

pub fn get_scoped(scope: &CacheScope, tool: &str, args: &Value) -> Option<String> {
    if !is_cacheable(tool) {
        return None;
    }
    let k = key(scope, tool, args);
    let mut guard = map().lock().ok()?;
    let now = Instant::now();
    // Opportunistic GC of a few expired entries.
    if guard.len() > 256 {
        guard.retain(|_, e| e.expires > now);
    }
    let e = guard.get(&k)?;
    if e.scope != *scope || e.tool != tool || e.args != *args {
        guard.remove(&k);
        return None;
    }
    if e.expires <= now {
        guard.remove(&k);
        return None;
    }
    Some(e.value.clone())
}

pub fn put(tool: &str, args: &Value, value: &str) {
    put_scoped(&CacheScope::global(), tool, args, value)
}

pub fn put_scoped(scope: &CacheScope, tool: &str, args: &Value, value: &str) {
    if !is_cacheable(tool) {
        return;
    }
    // Don't cache errors — agent should retry.
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("error:") || trimmed.starts_with("Error") {
        return;
    }
    let k = key(scope, tool, args);
    if let Ok(mut guard) = map().lock() {
        guard.insert(
            k,
            Entry {
                scope: scope.clone(),
                tool: tool.to_string(),
                args: args.clone(),
                value: value.to_string(),
                expires: Instant::now() + ttl_for(tool),
            },
        );
    }
}

fn is_path_ancestor(directory: &str, path: &str) -> bool {
    let normalize = |value: &str| value.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    let directory = normalize(directory);
    let path = normalize(path);
    path == directory || path.strip_prefix(&directory).is_some_and(|rest| rest.starts_with('/'))
}

#[allow(dead_code)]
pub fn invalidate(invalidation: &Invalidation) {
    invalidate_scoped(&CacheScope::global(), invalidation);
}

pub fn invalidate_scoped(scope: &CacheScope, invalidation: &Invalidation) {
    let Ok(mut guard) = map().lock() else { return };
    guard.retain(|_, entry| {
        if entry.scope != *scope {
            return true;
        }
        match invalidation {
            Invalidation::Skills => !matches!(entry.tool.as_str(), "skills_list" | "skill_view"),
            Invalidation::HostMemory { vps_id } => {
                !(entry.tool == "host_memory_get"
                    && entry.args.get("vps_id").and_then(Value::as_str) == Some(vps_id))
            }
            Invalidation::RemoteFile { vps_id, path } => {
                !(entry.tool == "read_file"
                    && entry.args.get("vps_id").and_then(Value::as_str) == Some(vps_id)
                    && entry.args.get("path").and_then(Value::as_str) == Some(path))
            }
            Invalidation::LocalFile { path } => {
                let matches_file = entry.tool == "local_read_file"
                    && entry.args.get("path").and_then(Value::as_str) == Some(path);
                let matches_dir = entry.tool == "local_list_dir"
                    && entry.args.get("path").and_then(Value::as_str)
                        .is_some_and(|dir| is_path_ancestor(dir, path));
                !(matches_file || matches_dir)
            }
        }
    });
}

/// Drop all entries (e.g. after major config change).
pub fn clear() {
    if let Ok(mut guard) = map().lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CACHE` is one process-global map, so tests that put/clear entries are not
    /// independent: run in parallel, one test's `clear()` wipes another's entries and
    /// both fail intermittently. Every test that touches the cache takes this lock, so
    /// they serialise against each other while the rest of the suite still runs in
    /// parallel. Poisoning is recovered from — one failing test must not cascade into
    /// "all cache tests panicked" and hide the original failure.
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Take the cache lock and hand back a pristine cache. Hold the guard for the whole
    /// test (`let _guard = cache_guard();`).
    fn cache_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        guard
    }

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
        let _guard = cache_guard();
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
        let _guard = cache_guard();
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
    fn targeted_invalidation_removes_only_related_entries() {
        let _guard = cache_guard();
        let remote = serde_json::json!({ "vps_id": "vps-a", "path": "/etc/app.conf" });
        let other_remote = serde_json::json!({ "vps_id": "vps-a", "path": "/etc/other.conf" });
        put("read_file", &remote, "old");
        put("read_file", &other_remote, "other");
        invalidate(&Invalidation::RemoteFile {
            vps_id: "vps-a".into(),
            path: "/etc/app.conf".into(),
        });
        assert_eq!(get("read_file", &remote), None);
        assert_eq!(get("read_file", &other_remote).as_deref(), Some("other"));
        clear();
    }

    #[test]
    fn local_invalidation_removes_file_and_parent_listing() {
        let _guard = cache_guard();
        let file = serde_json::json!({ "path": "C:/work/app.env" });
        let dir = serde_json::json!({ "path": "C:/work" });
        put("local_read_file", &file, "old");
        put("local_list_dir", &dir, "listing");
        invalidate(&Invalidation::LocalFile { path: "C:/work/app.env".into() });
        assert_eq!(get("local_read_file", &file), None);
        assert_eq!(get("local_list_dir", &dir), None);
        clear();
    }

    #[test]
    fn host_and_skill_invalidation_remove_their_reads() {
        let _guard = cache_guard();
        let host = serde_json::json!({ "vps_id": "vps-a" });
        let skill = serde_json::json!({ "category": "ops", "name": "deploy" });
        put("host_memory_get", &host, "profile");
        put("skill_view", &skill, "skill");
        put("skills_list", &serde_json::json!({}), "list");
        invalidate(&Invalidation::HostMemory { vps_id: "vps-a".into() });
        assert_eq!(get("host_memory_get", &host), None);
        invalidate(&Invalidation::Skills);
        assert_eq!(get("skill_view", &skill), None);
        assert_eq!(get("skills_list", &serde_json::json!({})), None);
        clear();
    }

    #[test]
    fn cache_scope_canonicalizes_targets_and_isolates_contexts() {
        let _guard = cache_guard();
        let home = std::path::Path::new("C:/agent");
        let first = CacheScope::new("session-a", Some("workspace-a"), &["vps-b".into(), "vps-a".into(), "vps-a".into()], home);
        let equivalent = CacheScope::new("session-a", Some("workspace-a"), &["vps-a".into(), "vps-b".into()], home);
        let other = CacheScope::new("session-b", Some("workspace-a"), &["vps-a".into(), "vps-b".into()], home);
        assert_eq!(first, equivalent);
        assert_ne!(first, other);

        clear();
        let args = serde_json::json!({ "path": "/etc/app.conf" });
        put_scoped(&first, "read_file", &args, "session-a value");
        assert_eq!(get_scoped(&equivalent, "read_file", &args).as_deref(), Some("session-a value"));
        assert_eq!(get_scoped(&other, "read_file", &args), None);
        clear();
    }

    #[test]
    fn directory_invalidation_respects_component_boundaries() {
        let _guard = cache_guard();
        let work = serde_json::json!({ "path": "C:/work" });
        let workspace = serde_json::json!({ "path": "C:/workspace" });
        put("local_list_dir", &work, "work");
        put("local_list_dir", &workspace, "workspace");
        invalidate(&Invalidation::LocalFile { path: "C:/work/app.env".into() });
        assert_eq!(get("local_list_dir", &work), None);
        assert_eq!(get("local_list_dir", &workspace).as_deref(), Some("workspace"));
        clear();
    }

    #[test]
    fn clear_removes_cached_entries() {
        let _guard = cache_guard();
        let args = serde_json::json!({ "path": "reset" });
        put("local_read_file", &args, "value");
        assert!(get("local_read_file", &args).is_some());
        clear();
        assert_eq!(get("local_read_file", &args), None);
    }
}
