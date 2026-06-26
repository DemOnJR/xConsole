//! AI agent subsystem: providers, registry, agent loop, tools, and the
//! Hermes-style soul / memory / context / skills / cron cores.

use std::path::PathBuf;

pub mod agent;
pub mod canvas_context;
pub mod context;
pub mod context_compact;
pub mod context_usage;
pub mod conversations;
pub mod cron;
pub mod edits;
pub mod hooks;
pub mod infra_tools;
pub mod interaction;
pub mod llama;
pub mod memory;
pub mod models;
pub mod provider;
pub mod providers;
pub mod vps_snapshot;
pub mod reflection;
pub mod registry;
pub mod safety;
pub mod skill_install;
pub mod skill_scan;
pub mod skills;
pub mod soul;
pub mod edge_tts;
pub mod parakeet;
pub mod piper;
pub mod text;
pub mod tools;
pub mod voice;
pub mod web_tools;
pub mod workspace_context;

/// Filesystem home for the agent's editable, Hermes-format files
/// (SOUL.md / MEMORY.md / USER.md / skills/ / cron/). Managed as Tauri state.
#[derive(Clone)]
pub struct AgentHome(pub PathBuf);

impl AgentHome {
    pub fn new(base: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&base);
        Self(base)
    }
    pub fn soul(&self) -> PathBuf {
        self.0.join("SOUL.md")
    }
    pub fn memory(&self) -> PathBuf {
        self.0.join("MEMORY.md")
    }
    pub fn user(&self) -> PathBuf {
        self.0.join("USER.md")
    }
    pub fn skills_dir(&self) -> PathBuf {
        self.0.join("skills")
    }
    pub fn projects_dir(&self) -> PathBuf {
        self.0.join("projects")
    }
    /// Per-workspace agent files (CONTEXT.md brief + MEMORY.md), one dir per workspace.
    pub fn workspaces_dir(&self) -> PathBuf {
        self.0.join("workspaces")
    }
}
