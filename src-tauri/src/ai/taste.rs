//! User working-style preferences (`TASTE.md`) — Command Code–inspired taste learning
//! for DevOps. Changes rarely, so the content is injected into the *static* system
//! prefix (cache-friendly). Reflection and explicit memory tools can append bullets.

use crate::ai::AgentHome;

pub const TASTE_MAX_CHARS: usize = 4000;

/// Merge cap for the user-profile slice of TASTE.md (matches the old USER.md cap).
pub const USER_MAX_CHARS: usize = 3000;

pub fn path(home: &AgentHome) -> std::path::PathBuf {
    home.0.join("TASTE.md")
}

pub fn load(home: &AgentHome) -> String {
    std::fs::read_to_string(path(home)).unwrap_or_default()
}

pub fn save(home: &AgentHome, content: &str) -> Result<(), String> {
    std::fs::write(path(home), content).map_err(|e| e.to_string())
}

/// Append a preference bullet (normalized to one line).
pub fn append(home: &AgentHome, entry: &str) -> Result<String, String> {
    let normalized = entry
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = normalized.trim_start_matches(['-', '*', '•']).trim();
    if normalized.is_empty() {
        return Err("taste entry is empty".into());
    }
    let mut content = load(home);
    if content.lines().any(|l| l.contains(normalized)) {
        return Ok(content);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("- ");
    content.push_str(normalized);
    content.push('\n');
    save(home, &content)?;
    Ok(content)
}

pub fn format_for_prompt(home: &AgentHome) -> String {
    let raw = load(home);
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let body = if t.len() <= TASTE_MAX_CHARS {
        t.to_string()
    } else {
        let mut cut = TASTE_MAX_CHARS;
        while !t.is_char_boundary(cut) && cut > 0 {
            cut -= 1;
        }
        format!("{}\n…(truncated)", t[..cut].trim())
    };
    format!(
        "# Preferences (TASTE.md)\n\
         User profile + working style. Follow these preferences when choosing commands, paths, \
         and how you report results:\n{body}"
    )
}

/// Read the merged user-profile slice of TASTE.md (the first `USER_MAX_CHARS`
/// chars, like the old USER.md). After consolidation, USER.md content lives in
/// TASTE.md; this keeps `memory::load_user` working for backward compatibility.
pub fn load_user_profile(home: &AgentHome) -> String {
    let raw = load(home);
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() <= USER_MAX_CHARS {
        t.to_string()
    } else {
        let mut cut = USER_MAX_CHARS;
        while !t.is_char_boundary(cut) && cut > 0 {
            cut -= 1;
        }
        format!("{}\n…(truncated)", t[..cut].trim())
    }
}
