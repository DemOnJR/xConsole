//! Built-in compact memory: `MEMORY.md` (durable facts) plus the user-profile
//! view of the consolidated `TASTE.md` store, mirroring Hermes' always-on
//! built-in memory. Injected into the volatile tier of the system prompt.

use crate::ai::AgentHome;

/// Keep the injected memory block compact (Hermes-style). Content beyond this is
/// truncated in the prompt (the file keeps everything; the agent compacts it).
pub const MEMORY_MAX_CHARS: usize = 6000;

pub fn load_memory(home: &AgentHome) -> String {
    read(home.memory().as_path())
}

pub fn save_memory(home: &AgentHome, content: &str) -> Result<(), String> {
    std::fs::write(home.memory(), content).map_err(|e| e.to_string())
}

/// Append a memory entry as one bullet per non-empty line, then return the new
/// contents.
pub fn append_memory(home: &AgentHome, entry: &str) -> Result<String, String> {
    let lines: Vec<String> = entry
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| strip_single_marker(l).to_string())
        .collect();
    if lines.is_empty() {
        return Err("memory entry is empty".into());
    }

    let mut content = load_memory(home);
    let mut added = false;
    for line in lines {
        if content.lines().any(|l| l.trim() == line) {
            continue; // dedup exact lines only
        }
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("- ");
        content.push_str(&line);
        content.push('\n');
        added = true;
    }
    if added {
        save_memory(home, &content)?;
    }
    Ok(content)
}

/// Strip a single leading bullet marker + space (never `--flag`'s dashes).
fn strip_single_marker(line: &str) -> &str {
    let l = line.trim_start();
    for m in ["- ", "* ", "• ", "-", "*", "•"] {
        if let Some(rest) = l.strip_prefix(m) {
            return rest.trim_start();
        }
    }
    l
}

/// One-time migration: fold any existing `USER.md` content into `TASTE.md`
/// (consolidated preferences store), then delete `USER.md`. Idempotent — no-op
/// when USER.md is absent. Call at startup before any prompt assembly.
pub fn migrate_user_into_taste(home: &AgentHome) {
    let user_path = home.user();
    if !user_path.exists() {
        return;
    }
    let content = read(&user_path);
    let merged;
    if content.trim().is_empty() {
        // Nothing to migrate — just remove the empty legacy file.
        let _ = std::fs::remove_file(&user_path);
        return;
    }
    let taste = crate::ai::taste::load(home);
    // Merge raw content (never flatten through append — that would mangle a
    // structured profile into one giant bullet line).
    merged = if taste.trim().is_empty() {
        content.trim().to_string()
    } else {
        format!("{}\n{}\n", taste.trim_end(), content.trim())
    };
    // Save FIRST; only delete USER.md once the merge is safely on disk.
    if crate::ai::taste::save(home, &merged).is_ok() {
        let _ = std::fs::remove_file(&user_path);
    }
}

/// Render the memory block for the volatile prompt tier. User-profile content
/// was consolidated into TASTE.md and is injected once via `taste::format_for_prompt`;
/// duplicating it here would send the same content twice per turn.
pub fn format_for_prompt(home: &AgentHome) -> String {
    let mem = load_memory(home);
    if mem.trim().is_empty() {
        return String::new();
    }
    format!(
        "# Persistent memory (MEMORY.md)\n{}",
        truncate(&mem, MEMORY_MAX_CHARS)
    )
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
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
