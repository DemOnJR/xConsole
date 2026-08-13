//! User working-style preferences (`TASTE.md`) — Command Code–inspired taste learning
//! for DevOps. Changes rarely, so the content is injected into the *static* system
//! prefix (cache-friendly). Reflection and explicit memory tools can append bullets.

use crate::ai::AgentHome;

pub const TASTE_MAX_CHARS: usize = 4000;

pub fn path(home: &AgentHome) -> std::path::PathBuf {
    home.0.join("TASTE.md")
}

pub fn load(home: &AgentHome) -> String {
    std::fs::read_to_string(path(home)).unwrap_or_default()
}

pub fn save(home: &AgentHome, content: &str) -> Result<(), String> {
    std::fs::write(path(home), content).map_err(|e| e.to_string())
}

/// Append a preference (normalized to one bullet per non-empty line).
pub fn append(home: &AgentHome, entry: &str) -> Result<String, String> {
    let lines: Vec<String> = entry
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| strip_single_marker(l).to_string())
        .collect();
    if lines.is_empty() {
        return Err("taste entry is empty".into());
    }
    let mut content = load(home);
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
        save(home, &content)?;
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
