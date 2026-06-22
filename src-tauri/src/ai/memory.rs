//! Built-in compact memory: `MEMORY.md` (durable facts) and `USER.md` (profile),
//! mirroring Hermes' always-on built-in memory store. Injected into the volatile
//! tier of the system prompt.

use crate::ai::AgentHome;

/// Keep the injected memory block compact (Hermes-style). Content beyond this is
/// truncated in the prompt (the file keeps everything; the agent compacts it).
pub const MEMORY_MAX_CHARS: usize = 6000;
pub const USER_MAX_CHARS: usize = 3000;

pub fn load_memory(home: &AgentHome) -> String {
    read(home.memory().as_path())
}

pub fn load_user(home: &AgentHome) -> String {
    read(home.user().as_path())
}

pub fn save_memory(home: &AgentHome, content: &str) -> Result<(), String> {
    std::fs::write(home.memory(), content).map_err(|e| e.to_string())
}

pub fn save_user(home: &AgentHome, content: &str) -> Result<(), String> {
    std::fs::write(home.user(), content).map_err(|e| e.to_string())
}

/// Append a memory entry as a bullet line, then return the new contents.
pub fn append_memory(home: &AgentHome, entry: &str) -> Result<String, String> {
    let mut content = load_memory(home);
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("- ");
    content.push_str(entry.trim());
    content.push('\n');
    save_memory(home, &content)?;
    Ok(content)
}

/// Render the memory + user blocks for the volatile prompt tier.
pub fn format_for_prompt(home: &AgentHome) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mem = load_memory(home);
    if !mem.trim().is_empty() {
        parts.push(format!(
            "# Persistent memory (MEMORY.md)\n{}",
            truncate(&mem, MEMORY_MAX_CHARS)
        ));
    }
    let user = load_user(home);
    if !user.trim().is_empty() {
        parts.push(format!(
            "# User profile (USER.md)\n{}",
            truncate(&user, USER_MAX_CHARS)
        ));
    }
    parts.join("\n\n")
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
