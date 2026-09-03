//! AI agent subsystem: providers, registry, agent loop, tools, and the
//! soul / memory / context / skills / cron cores.

use std::path::PathBuf;

pub mod agent;
pub mod analytics;
pub mod autoresearch;
pub mod canvas_context;
pub mod file_ops;
pub mod file_state;
pub mod todos;
pub mod transcript;
pub mod cli_agent;
pub mod consent;
pub mod context;
pub mod context_compact;
pub mod context_usage;
pub mod conversations;
pub mod cost;
pub mod cron;
pub mod edits;
pub mod escalation;
pub mod gpu;
pub mod goal;
pub mod hooks;
pub mod host_memory;
pub mod image_gen;
pub mod infra_tools;
pub mod irreversible;
pub mod interaction;
pub mod learn;
pub mod jobs;
pub mod list_models;
pub mod llama;
pub mod memory;
pub mod models;
pub mod output_compress;
pub mod provider;
pub mod providers;
pub mod redaction;
pub mod vps_snapshot;
pub mod reflection;
pub mod registry;
pub mod remote;
pub mod report;
pub mod repo;
pub mod metrics_tools;
pub mod remote_tools;
pub mod safety;
pub mod scope;
pub mod skill_install;
pub mod skill_scan;
pub mod skills;
pub mod soul;
pub mod taste;
pub mod tool_cache;
pub mod edge_tts;
pub mod parakeet;
pub mod persona;
pub mod persona_tools;
pub mod pr_guard;
pub mod piper;
pub mod prefix_telemetry;
pub mod text;
pub mod tools;
pub mod vision;
pub mod voice;
pub mod web_tools;
pub mod workspace_context;

/// Filesystem home for the agent's editable, Hermes-format files
/// (SOUL.md / MEMORY.md / TASTE.md / skills/ / cron/). Managed as Tauri state.
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
    /// Legacy USER.md path — read only by the one-time consolidation migration.
    pub fn user(&self) -> PathBuf {
        self.0.join("USER.md")
    }
    pub fn taste(&self) -> PathBuf {
        self.0.join("TASTE.md")
    }
    #[allow(dead_code)]
    pub fn hosts_dir(&self) -> PathBuf {
        self.0.join("hosts")
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
