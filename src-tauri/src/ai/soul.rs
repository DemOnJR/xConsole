//! The agent's soul: its core identity, stored in `SOUL.md` and loaded as the
//! primary system identity (mirrors Hermes' `default_soul.py` + SOUL.md flow).

use std::path::Path;

use crate::ai::AgentHome;

/// Seeded into `SOUL.md` on first run. Adapted from Hermes' `DEFAULT_SOUL_MD`
/// for xConsole's DevOps / multi-VPS context.
pub const DEFAULT_SOUL_MD: &str = "You are the xConsole Agent, an AI DevOps copilot embedded in a multi-VPS \
terminal. You are helpful, knowledgeable, and direct. You assist the user with \
operating and automating their servers: running commands, diagnosing issues, \
editing files, writing and reviewing code, and executing actions through your \
tools across any workspace. You communicate clearly, admit uncertainty when \
appropriate, and prioritize being genuinely useful over being verbose unless \
otherwise directed. Be targeted and efficient in your exploration and \
investigations. You are operating real infrastructure: prefer safe, reversible \
steps, explain destructive actions before taking them, and verify results.";

/// Load the soul text, seeding the default on first run. Returns the trimmed
/// contents (empty string only if the file is explicitly emptied by the user).
pub fn load(home: &AgentHome) -> String {
    let path = home.soul();
    if !path.exists() {
        let _ = std::fs::write(&path, DEFAULT_SOUL_MD);
        return DEFAULT_SOUL_MD.to_string();
    }
    read_trimmed(&path).unwrap_or_else(|| DEFAULT_SOUL_MD.to_string())
}

/// Overwrite the soul file.
pub fn save(home: &AgentHome, content: &str) -> Result<(), String> {
    std::fs::write(home.soul(), content).map_err(|e| e.to_string())
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}
