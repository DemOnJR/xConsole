//! Tauri commands for the AI agent. The agent loop, chat, and tool dispatch are
//! added in later phases; this file currently exposes provider CLI login.

use tauri::{AppHandle, Emitter, State};

use crate::ai::agent;
use crate::ai::provider::{ChatMessage, StreamEvent};
use crate::ai::providers::cli;
use crate::ai::cron::{self, CronContext, CronRunning};
use crate::ai::safety::ApprovalRegistry;
use crate::ai::tools::ToolContext;
use crate::ai::AgentHome;
use crate::ssh::SessionManager;
use crate::storage::models::{
    AgentApproval, AgentConversation, AgentConversationInput, AgentConversationMeta, CronJob,
    CronJobInput,
};
use crate::storage::Db;

/// Event channel name for streamed CLI-login output.
pub fn login_event(provider_id: &str) -> String {
    format!("ai://login/{provider_id}")
}

/// Event channel name for a chat session's streamed output.
pub fn chat_event(session_id: &str) -> String {
    format!("ai://chat/{session_id}")
}

/// Resolve the active safety mode, defaulting to the safest interactive one.
fn safety_mode(db: &Db) -> String {
    db.get_setting("agent.safety_mode")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "approve".to_string())
}

/// Run one agent turn. Streams events to `ai://chat/<session_id>` and returns the
/// final assistant message for the frontend to append.
#[tauri::command]
pub async fn ai_chat(
    app: AppHandle,
    db: State<'_, Db>,
    home: State<'_, AgentHome>,
    sessions: State<'_, SessionManager>,
    approvals: State<'_, ApprovalRegistry>,
    session_id: String,
    messages: Vec<ChatMessage>,
    provider_id: Option<String>,
    targets: Vec<String>,
) -> Result<ChatMessage, String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let event = chat_event(&session_id);
    let app2 = app.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app2.emit(&event, ev);
        }
    });

    let db_inner = db.inner().clone();
    let tc = ToolContext {
        app: app.clone(),
        db: db_inner.clone(),
        sessions: sessions.inner().clone(),
        home: home.inner().clone(),
        approvals: approvals.inner().clone(),
        session_id,
        targets,
        safety: safety_mode(&db_inner),
    };

    agent::run_turn(&tc, provider_id.filter(|s| !s.is_empty()), messages, &tx).await
}

#[tauri::command]
pub fn list_agent_conversations(db: State<'_, Db>) -> Result<Vec<AgentConversationMeta>, String> {
    db.list_agent_conversations(50)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent_conversation(
    db: State<'_, Db>,
    id: String,
) -> Result<Option<AgentConversation>, String> {
    db.get_agent_conversation(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_agent_conversation(
    db: State<'_, Db>,
    input: AgentConversationInput,
) -> Result<AgentConversation, String> {
    let conv = db.upsert_agent_conversation(&input).map_err(|e| e.to_string())?;
    let _ = db.set_setting("agent.last_conversation", &input.id);
    Ok(conv)
}

#[tauri::command]
pub fn delete_agent_conversation(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_agent_conversation(&id).map_err(|e| e.to_string())
}

/// Resolve a pending command approval (from the UI).
#[tauri::command]
pub fn agent_resolve_approval(
    db: State<'_, Db>,
    approvals: State<'_, ApprovalRegistry>,
    id: String,
    approved: bool,
) -> Result<(), String> {
    let _ = db.resolve_approval(&id, if approved { "approved" } else { "denied" });
    approvals.resolve(&id, approved);
    Ok(())
}

/// List approvals still awaiting a decision.
#[tauri::command]
pub fn list_pending_approvals(db: State<'_, Db>) -> Result<Vec<AgentApproval>, String> {
    db.list_pending_approvals().map_err(|e| e.to_string())
}

// ----- Soul / Memory / User files -----

/// The agent's editable Hermes-format documents.
#[derive(serde::Serialize)]
pub struct AgentDocs {
    pub soul: String,
    pub memory: String,
    pub user: String,
}

#[tauri::command]
pub fn get_agent_docs(home: State<'_, AgentHome>) -> AgentDocs {
    AgentDocs {
        soul: crate::ai::soul::load(&home),
        memory: crate::ai::memory::load_memory(&home),
        user: crate::ai::memory::load_user(&home),
    }
}

#[tauri::command]
pub fn save_soul(home: State<'_, AgentHome>, content: String) -> Result<(), String> {
    crate::ai::soul::save(&home, &content)
}

#[tauri::command]
pub fn save_memory_doc(home: State<'_, AgentHome>, content: String) -> Result<(), String> {
    crate::ai::memory::save_memory(&home, &content)
}

#[tauri::command]
pub fn save_user_doc(home: State<'_, AgentHome>, content: String) -> Result<(), String> {
    crate::ai::memory::save_user(&home, &content)
}

// ----- Skills -----

#[tauri::command]
pub fn list_skills(home: State<'_, AgentHome>) -> Vec<crate::ai::skills::Skill> {
    crate::ai::skills::discover(&home)
}

#[tauri::command]
pub fn get_skill(
    home: State<'_, AgentHome>,
    category: String,
    name: String,
) -> Option<String> {
    crate::ai::skills::read_skill(&home, &category, &name)
}

#[tauri::command]
pub fn save_skill(
    home: State<'_, AgentHome>,
    category: String,
    name: String,
    content: String,
) -> Result<(), String> {
    crate::ai::skills::save_skill(&home, &category, &name, &content)
}

#[tauri::command]
pub fn delete_skill(
    home: State<'_, AgentHome>,
    category: String,
    name: String,
) -> Result<(), String> {
    crate::ai::skills::delete_skill(&home, &category, &name)
}

// ----- Cron -----

#[tauri::command]
pub fn list_cron_jobs(db: State<'_, Db>) -> Result<Vec<CronJob>, String> {
    db.list_cron_jobs().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_cron_job(db: State<'_, Db>, input: CronJobInput) -> Result<CronJob, String> {
    db.upsert_cron_job(&input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_cron_job(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_cron_job(&id).map_err(|e| e.to_string())
}

/// Run a cron job immediately (streams to `ai://cron/<id>`).
#[tauri::command]
pub async fn run_cron_job(
    app: AppHandle,
    db: State<'_, Db>,
    sessions: State<'_, SessionManager>,
    home: State<'_, AgentHome>,
    approvals: State<'_, ApprovalRegistry>,
    running: State<'_, CronRunning>,
    id: String,
) -> Result<(), String> {
    let job = db
        .list_cron_jobs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|j| j.id == id)
        .ok_or_else(|| "cron job not found".to_string())?;

    let ctx = CronContext {
        app,
        db: db.inner().clone(),
        sessions: sessions.inner().clone(),
        home: home.inner().clone(),
        approvals: approvals.inner().clone(),
        running: running.inner().clone(),
    };
    cron::run_job(&ctx, &job).await;
    Ok(())
}

/// Spawn a CLI provider's login flow, streaming output to `ai://login/<id>`.
#[tauri::command]
pub async fn ai_cli_login(
    app: AppHandle,
    db: State<'_, Db>,
    provider_id: String,
) -> Result<String, String> {
    let provider = db
        .get_provider(&provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "provider not found".to_string())?;

    if !cli::is_cli_kind(&provider.kind) {
        return Err("login is only available for CLI providers (Cursor, Codex, OpenCode)".to_string());
    }
    let bin = provider
        .bin_path
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cli::CliProvider::default_bin(&provider.kind));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let event = login_event(&provider_id);
    let app2 = app.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app2.emit(&event, ev);
        }
    });

    cli::login(&provider.kind, &bin, Some(&tx)).await
}
