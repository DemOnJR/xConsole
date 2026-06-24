//! Per-workspace project context for the agent.
//!
//! When a workspace is active, the agent is given: an auto-maintained, user-editable
//! **brief** (`CONTEXT.md`), **workspace-scoped memory** (`MEMORY.md`), and the
//! project's own agent files (`CLAUDE.md` / `AGENTS.md` / `.claude/*.md`) read from
//! the workspace's configured project location — a local folder or a path on a VPS.
//! The brief + memory live under `AgentHome/workspaces/<id>/`; the project files are
//! read live from wherever the project actually is.

use serde::Deserialize;

use crate::ai::AgentHome;
use crate::ssh::{shell_quote, SessionManager};
use crate::storage::Db;

const BRIEF_MAX_CHARS: usize = 6000;
const MEMORY_MAX_CHARS: usize = 6000;
const PROJECT_FILES_MAX_CHARS: usize = 8000;

/// Top-level agent context files to look for in a project root.
const AGENT_FILE_CANDIDATES: &[&str] = &[
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    "AGENT.md",
    ".cursorrules",
];

/// Parsed `workspace.project_json`: where the workspace's project lives.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectLocation {
    /// "local" | "vps"
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub vps_id: Option<String>,
}

fn ws_dir(home: &AgentHome, workspace_id: &str) -> std::path::PathBuf {
    // Workspace ids are UUIDs; sanitize defensively so they can't escape the tree.
    let safe: String = workspace_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    home.workspaces_dir().join(safe)
}

fn brief_path(home: &AgentHome, id: &str) -> std::path::PathBuf {
    ws_dir(home, id).join("CONTEXT.md")
}

fn memory_path(home: &AgentHome, id: &str) -> std::path::PathBuf {
    ws_dir(home, id).join("MEMORY.md")
}

pub fn load_brief(home: &AgentHome, id: &str) -> String {
    std::fs::read_to_string(brief_path(home, id)).unwrap_or_default()
}

pub fn save_brief(home: &AgentHome, id: &str, content: &str) -> Result<(), String> {
    std::fs::create_dir_all(ws_dir(home, id)).map_err(|e| e.to_string())?;
    std::fs::write(brief_path(home, id), content).map_err(|e| e.to_string())
}

pub fn load_memory(home: &AgentHome, id: &str) -> String {
    std::fs::read_to_string(memory_path(home, id)).unwrap_or_default()
}

/// Append a workspace-scoped memory entry as one bullet (mirrors [`crate::ai::memory::append_memory`]).
pub fn append_memory(home: &AgentHome, id: &str, entry: &str) -> Result<(), String> {
    let normalized = entry
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = normalized.trim_start_matches(['-', '*', '•']).trim();
    if normalized.is_empty() {
        return Err("memory entry is empty".into());
    }
    std::fs::create_dir_all(ws_dir(home, id)).map_err(|e| e.to_string())?;
    let path = memory_path(home, id);
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("- ");
    content.push_str(normalized);
    content.push('\n');
    std::fs::write(&path, &content).map_err(|e| e.to_string())
}

fn parse_location(json: &str) -> Option<ProjectLocation> {
    serde_json::from_str::<ProjectLocation>(json)
        .ok()
        .filter(|l| l.path.as_deref().map(|p| !p.trim().is_empty()).unwrap_or(false))
}

/// Read the project's own agent context files from its location (local or VPS).
async fn load_project_agent_files(sessions: &SessionManager, loc: &ProjectLocation) -> String {
    let Some(root) = loc.path.as_deref().filter(|p| !p.trim().is_empty()) else {
        return String::new();
    };
    let text = match loc.kind.as_str() {
        "vps" => match loc.vps_id.as_deref() {
            Some(vps_id) if !vps_id.is_empty() => load_vps_files(sessions, vps_id, root).await,
            _ => String::new(),
        },
        _ => load_local_files(root),
    };
    truncate(&text, PROJECT_FILES_MAX_CHARS)
}

fn load_local_files(root: &str) -> String {
    let base = std::path::Path::new(root);
    let mut parts: Vec<String> = Vec::new();
    for cand in AGENT_FILE_CANDIDATES {
        if let Ok(text) = std::fs::read_to_string(base.join(cand)) {
            if !text.trim().is_empty() {
                parts.push(format!("### {cand}\n{}", text.trim()));
            }
        }
    }
    // Plus any markdown under .claude/.
    if let Ok(rd) = std::fs::read_dir(base.join(".claude")) {
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.extension().and_then(|x| x.to_str()) == Some("md") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if !text.trim().is_empty() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        parts.push(format!("### .claude/{name}\n{}", text.trim()));
                    }
                }
            }
        }
    }
    parts.join("\n\n")
}

async fn load_vps_files(sessions: &SessionManager, vps_id: &str, root: &str) -> String {
    // One round-trip: cd into the root, print each present agent file with a marker.
    let cmd = format!(
        "cd {} 2>/dev/null && for f in CLAUDE.md CLAUDE.local.md AGENTS.md AGENT.md .cursorrules .claude/*.md; do \
         [ -f \"$f\" ] && printf '<<<FILE %s>>>\\n' \"$f\" && cat \"$f\"; done",
        shell_quote(root)
    );
    let out = match sessions.run_command(vps_id, &cmd).await {
        Ok(o) => o,
        Err(_) => return String::new(),
    };
    // Reformat the marker stream into labeled sections.
    let mut parts: Vec<String> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut buf = String::new();
    for line in out.stdout.lines() {
        if let Some(name) = line.strip_prefix("<<<FILE ").and_then(|s| s.strip_suffix(">>>")) {
            if let Some(prev) = current_name.take() {
                if !buf.trim().is_empty() {
                    parts.push(format!("### {prev}\n{}", buf.trim()));
                }
            }
            current_name = Some(name.to_string());
            buf.clear();
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if let Some(prev) = current_name {
        if !buf.trim().is_empty() {
            parts.push(format!("### {prev}\n{}", buf.trim()));
        }
    }
    parts.join("\n\n")
}

/// Assemble the full per-workspace context block for the system prompt, or `None`
/// when the workspace has no brief, memory, or readable project files.
pub async fn build_workspace_block(
    home: &AgentHome,
    db: &Db,
    sessions: &SessionManager,
    workspace_id: &str,
) -> Option<String> {
    let ws = db.get_workspace(workspace_id).ok().flatten()?;

    let brief = load_brief(home, workspace_id);
    let memory = load_memory(home, workspace_id);
    let files = match ws.project_json.as_deref().and_then(parse_location) {
        Some(loc) => {
            let where_ = match loc.kind.as_str() {
                "vps" => format!(
                    "VPS {} : {}",
                    loc.vps_id.clone().unwrap_or_default(),
                    loc.path.clone().unwrap_or_default()
                ),
                _ => loc.path.clone().unwrap_or_default(),
            };
            let body = load_project_agent_files(sessions, &loc).await;
            if body.trim().is_empty() {
                String::new()
            } else {
                format!("## Project files (from {where_})\n{body}")
            }
        }
        None => String::new(),
    };

    let mut parts = vec![format!("# Active workspace: {}", ws.name)];
    parts.push(
        "This is the project the user is working in. Use this context; keep the brief current \
         with set_project_brief; save durable project facts with the memory tool."
            .to_string(),
    );
    if brief.trim().is_empty() {
        // First task in this workspace and no brief yet — bootstrap one (like `init`).
        parts.push(
            "## No project brief yet — create one\nThis workspace has no project brief. As part of \
             handling the user's first task here, take a moment to learn what this workspace is \
             about: if it has a configured project folder, read its README and key files and skim \
             the directory layout and any package/build manifests; otherwise look at the servers / \
             open panels it contains and what the user is doing. Then call set_project_brief with a \
             concise brief — purpose, stack (or servers/roles), important paths, how to run/build/ \
             test, and conventions — kept to roughly a screenful. Do this alongside the user's \
             actual request, not instead of it."
                .to_string(),
        );
    } else {
        parts.push(format!("## Project brief\n{}", truncate(&brief, BRIEF_MAX_CHARS)));
    }
    if !memory.trim().is_empty() {
        parts.push(format!(
            "## Workspace memory\n{}",
            truncate(&memory, MEMORY_MAX_CHARS)
        ));
    }
    if !files.is_empty() {
        parts.push(files);
    }
    Some(parts.join("\n\n"))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.trim().to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) && cut > 0 {
        cut -= 1;
    }
    format!("{}\n…(truncated)", s[..cut].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_location_requires_path() {
        assert!(parse_location(r#"{"kind":"local","path":"/x"}"#).is_some());
        assert!(parse_location(r#"{"kind":"local","path":""}"#).is_none());
        assert!(parse_location(r#"{"kind":"vps"}"#).is_none());
        assert!(parse_location("not json").is_none());
    }
}
