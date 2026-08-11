//! Per-VPS institutional memory (Xirp-style dossiers).
//!
//! Layout:
//! ```text
//! agent/hosts/<vps_id>/
//!   PROFILE.md   — role, OS, stack, services
//!   MEMORY.md    — durable facts about THIS host
//!   EVENTS.jsonl — optional append-only incident log
//! ```
//!
//! Only selected targets are injected into the dynamic context block each turn.

use crate::ai::AgentHome;
use crate::storage::Db;

const PROFILE_MAX: usize = 2500;
const MEMORY_MAX: usize = 3500;

fn host_dir(home: &AgentHome, vps_id: &str) -> std::path::PathBuf {
    // Sanitize id for filesystem (UUIDs are safe; belt-and-suspenders).
    let safe: String = vps_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    home.0.join("hosts").join(safe)
}

pub fn ensure_host_dir(home: &AgentHome, vps_id: &str) -> Result<std::path::PathBuf, String> {
    let dir = host_dir(home, vps_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn load_profile(home: &AgentHome, vps_id: &str) -> String {
    std::fs::read_to_string(host_dir(home, vps_id).join("PROFILE.md")).unwrap_or_default()
}

pub fn load_memory(home: &AgentHome, vps_id: &str) -> String {
    std::fs::read_to_string(host_dir(home, vps_id).join("MEMORY.md")).unwrap_or_default()
}

pub fn save_profile(home: &AgentHome, vps_id: &str, content: &str) -> Result<(), String> {
    let dir = ensure_host_dir(home, vps_id)?;
    std::fs::write(dir.join("PROFILE.md"), content).map_err(|e| e.to_string())
}

pub fn save_memory(home: &AgentHome, vps_id: &str, content: &str) -> Result<(), String> {
    let dir = ensure_host_dir(home, vps_id)?;
    std::fs::write(dir.join("MEMORY.md"), content).map_err(|e| e.to_string())
}

pub fn append_memory(home: &AgentHome, vps_id: &str, entry: &str) -> Result<String, String> {
    let normalized = entry
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = normalized.trim_start_matches(['-', '*', '•']).trim();
    if normalized.is_empty() {
        return Err("host memory entry is empty".into());
    }
    let mut content = load_memory(home, vps_id);
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("- ");
    content.push_str(normalized);
    content.push('\n');
    save_memory(home, vps_id, &content)?;
    Ok(content)
}

/// Append a JSONL event (best-effort; failures are ignored by callers that only log).
#[allow(dead_code)]
pub fn append_event(home: &AgentHome, vps_id: &str, event: &serde_json::Value) {
    let Ok(dir) = ensure_host_dir(home, vps_id) else {
        return;
    };
    let path = dir.join("EVENTS.jsonl");
    let line = serde_json::to_string(event).unwrap_or_default();
    if line.is_empty() {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.trim().to_string()
    } else {
        let mut cut = max;
        while !s.is_char_boundary(cut) && cut > 0 {
            cut -= 1;
        }
        format!("{}\n…(truncated)", s[..cut].trim())
    }
}

/// Format dossiers for the selected target ids (dynamic prompt block).
pub fn format_for_prompt(home: &AgentHome, db: &Db, target_ids: &[String]) -> String {
    if target_ids.is_empty() {
        return String::new();
    }
    let mut sections: Vec<String> = Vec::new();
    for id in target_ids {
        let vps = db.get_vps(id).ok().flatten();
        let name = vps
            .as_ref()
            .map(|v| format!("{} ({})", v.name, v.host))
            .unwrap_or_else(|| id.clone());
        let profile = load_profile(home, id);
        let mem = load_memory(home, id);
        if profile.trim().is_empty() && mem.trim().is_empty() {
            continue;
        }
        let mut body = format!("## Host: {name}\n_id: {id}_");
        if !profile.trim().is_empty() {
            body.push_str("\n\n### Profile\n");
            body.push_str(&truncate(&profile, PROFILE_MAX));
        }
        if !mem.trim().is_empty() {
            body.push_str("\n\n### Memory\n");
            body.push_str(&truncate(&mem, MEMORY_MAX));
        }
        sections.push(body);
    }
    if sections.is_empty() {
        return String::new();
    }
    format!(
        "# Host dossiers (selected VPS)\n\
         Prefer facts below over guessing or web search for these machines.\n\n{}",
        sections.join("\n\n")
    )
}
