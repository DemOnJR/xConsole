use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::storage::Db;

static SYSTEM_HOLDER: Mutex<Option<sysinfo::System>> = Mutex::new(None);
static CACHED_ANALYTICS: RwLock<Option<(AgentAnalytics, Instant)>> = RwLock::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct CachePoint {
    pub ts: String,
    pub session: String,
    pub prompt: u32,
    pub hit: u32,
    pub miss: u32,
    /// Tokens written to the provider cache on this request. Billed at 1.25x
    /// (5m TTL) or 2.0x (1h TTL) of the input rate — neither a hit nor free.
    pub written: u32,
    pub pct: i32,
    pub model: String,
    pub provider: String,
    /// "5m" or "1h".
    pub ttl: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCount {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationStat {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub user_turns: u32,
    pub tool_calls: u32,
    pub tools: Vec<ToolCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceSnapshot {
    pub ts: String,
    pub cpu_pct: f32,
    pub ram_mb: u64,
    pub ram_total_mb: u64,
    pub process_ram_mb: u64,
    pub gpu_pct: Option<f32>,
    pub gpu_mem_mb: Option<u64>,
    pub gpu_mem_total_mb: Option<u64>,
    pub gpu_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAnalytics {
    pub cache: Vec<CachePoint>,
    /// Token-weighted hit rate: `sum(hit) / sum(hit + miss + written)`.
    pub cache_avg_pct: f32,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub cache_written_tokens: u64,
    pub conversations: Vec<ConversationStat>,
    pub tools_all: Vec<ToolCount>,
    pub resource: ResourceSnapshot,
}

pub fn collect(db: &Db) -> AgentAnalytics {
    // Check if we have a fresh cached analytics structure (< 3s old)
    if let Ok(guard) = CACHED_ANALYTICS.read() {
        if let Some((cached, instant)) = guard.as_ref() {
            if instant.elapsed() < Duration::from_secs(3) {
                let mut fast_clone = cached.clone();
                fast_clone.resource = resource_snapshot();
                return fast_clone;
            }
        }
    }

    let cache = read_cache_log(400);
    // Token-weighted, not a mean of per-request percentages. Averaging the
    // percentages treats a 40-token request and a 400K-token full rewrite as
    // equals, so one cheap high-hit turn hid the expensive one.
    let cache_hit_tokens: u64 = cache.iter().map(|p| p.hit as u64).sum();
    let cache_miss_tokens: u64 = cache.iter().map(|p| p.miss as u64).sum();
    let cache_written_tokens: u64 = cache.iter().map(|p| p.written as u64).sum();
    let cache_avg_pct = (crate::ai::cost::weighted_cache_hit_rate(
        cache.iter().map(|p| (p.hit, p.miss, p.written)),
    ) * 100.0) as f32;
    let conversations = conversation_stats(db, 12);
    let mut all: BTreeMap<String, u32> = BTreeMap::new();
    for c in &conversations {
        for t in &c.tools {
            *all.entry(t.name.clone()).or_default() += t.count;
        }
    }
    let mut tools_all: Vec<ToolCount> = all
        .into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    tools_all.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

    let full = AgentAnalytics {
        cache,
        cache_avg_pct,
        cache_hit_tokens,
        cache_miss_tokens,
        cache_written_tokens,
        conversations,
        tools_all,
        resource: resource_snapshot(),
    };

    if let Ok(mut guard) = CACHED_ANALYTICS.write() {
        *guard = Some((full.clone(), Instant::now()));
    }

    full
}

fn read_cache_log(max: usize) -> Vec<CachePoint> {
    let Some(dir) = crate::dirs_next_app_data() else {
        return Vec::new();
    };
    let path = dir.join("cache.jsonl");
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut lines: Vec<String> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect();
    if lines.len() > max {
        lines = lines.split_off(lines.len() - max);
    }
    lines
        .iter()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(CachePoint {
                ts: v.get("ts")?.as_str()?.to_string(),
                session: v.get("session").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                prompt: v.get("prompt").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                hit: v.get("hit").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                miss: v.get("miss").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                written: v.get("written").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                pct: v.get("pct").and_then(|x| x.as_i64()).unwrap_or(0) as i32,
                model: v.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                provider: v.get("provider").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                ttl: v.get("ttl").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            })
        })
        .collect()
}

fn conversation_stats(db: &Db, limit: i64) -> Vec<ConversationStat> {
    let Ok(metas) = db.list_agent_conversations(limit) else {
        return Vec::new();
    };
    metas
        .into_iter()
        .filter_map(|meta| {
            let full = db.get_agent_conversation(&meta.id).ok().flatten()?;
            let msgs: Vec<serde_json::Value> =
                serde_json::from_str(&full.messages_json).unwrap_or_default();
            let mut user_turns = 0u32;
            let mut tool_calls = 0u32;
            let mut counts: BTreeMap<String, u32> = BTreeMap::new();
            for m in &msgs {
                if m.get("role").and_then(|r| r.as_str()) == Some("user") {
                    user_turns += 1;
                }
                if let Some(acts) = m.get("activity").and_then(|a| a.as_array()) {
                    for a in acts {
                        let name = a
                            .get("tool")
                            .and_then(|t| t.as_str())
                            .filter(|s| !s.is_empty())
                            .or_else(|| a.get("kind").and_then(|t| t.as_str()))
                            .unwrap_or("other");
                        if name == "status" || name == "command" {
                            continue;
                        }
                        tool_calls += 1;
                        *counts.entry(name.to_string()).or_default() += 1;
                    }
                }
            }
            let mut tools: Vec<ToolCount> = counts
                .into_iter()
                .map(|(name, count)| ToolCount { name, count })
                .collect();
            tools.sort_by(|a, b| b.count.cmp(&a.count));
            tools.truncate(8);
            Some(ConversationStat {
                id: meta.id,
                title: meta.title,
                updated_at: meta.updated_at.unwrap_or_default(),
                user_turns,
                tool_calls,
                tools,
            })
        })
        .collect()
}

pub fn resource_snapshot() -> ResourceSnapshot {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let pid = Pid::from_u32(std::process::id());

    let mut guard = SYSTEM_HOLDER.lock().unwrap_or_else(|e| e.into_inner());
    let sys = guard.get_or_insert_with(|| {
        let mut s = System::new();
        s.refresh_memory();
        s.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        s
    });

    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    let (process_ram_mb, cpu_pct) = sys
        .process(pid)
        .map(|p| (p.memory() / (1024 * 1024), p.cpu_usage()))
        .unwrap_or((0, 0.0));

    let gpu = crate::ai::gpu::snapshot();
    ResourceSnapshot {
        ts: chrono::Utc::now().to_rfc3339(),
        cpu_pct,
        ram_mb: sys.used_memory() / (1024 * 1024),
        ram_total_mb: sys.total_memory() / (1024 * 1024),
        process_ram_mb,
        gpu_pct: gpu.util_pct,
        gpu_mem_mb: gpu.mem_used_mb,
        gpu_mem_total_mb: gpu.mem_total_mb,
        gpu_name: gpu.name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(hit: u32, miss: u32, written: u32) -> CachePoint {
        CachePoint {
            ts: String::new(),
            session: String::new(),
            prompt: miss,
            hit,
            miss,
            written,
            pct: if hit + miss + written == 0 {
                0
            } else {
                ((hit as f64 / (hit + miss + written) as f64) * 100.0).round() as i32
            },
            model: String::new(),
            provider: String::new(),
            ttl: String::new(),
        }
    }

    #[test]
    fn average_hit_rate_is_token_weighted_and_counts_writes() {
        // One tiny perfect request and one huge full rewrite. Averaging the
        // per-request percentages reports 50%; the truth is ~0.1%.
        let rows = vec![point(100, 0, 0), point(0, 0, 100_000)];
        let unweighted =
            rows.iter().map(|p| p.pct as f32).sum::<f32>() / rows.len() as f32;
        let weighted = crate::ai::cost::weighted_cache_hit_rate(
            rows.iter().map(|p| (p.hit, p.miss, p.written)),
        ) * 100.0;
        assert_eq!(unweighted, 50.0);
        assert!(weighted < 0.2, "weighted={weighted}");
    }
}
