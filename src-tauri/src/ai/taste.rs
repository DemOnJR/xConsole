//! User working-style preferences (`TASTE.md`) — dynamic taste learning
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
    use crate::ai::text::BulletAppend;
    let existing = load(home);
    match crate::ai::text::append_bullets(&existing, entry) {
        BulletAppend::Empty => Err("taste entry is empty".into()),
        BulletAppend::Unchanged => Ok(existing),
        BulletAppend::Updated(content) => {
            save(home, &content)?;
            Ok(content)
        }
    }
}

pub fn format_for_prompt(home: &AgentHome) -> String {
    let raw = load(home);
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    // Preferences are appended, so the most recently learned ones are at the end —
    // keep those when the file outgrows the budget (see `text::keep_newest`).
    let body = crate::ai::text::keep_newest(t, TASTE_MAX_CHARS);
    format!(
        "# Preferences (TASTE.md)\n\
         User profile + working style. Follow these preferences when choosing commands, paths, \
         and how you report results:\n{body}"
    )
}
