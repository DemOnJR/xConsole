//! Tauri commands for the AI agent. The agent loop, chat, and tool dispatch are
//! added in later phases; this file currently exposes provider CLI login.

use tauri::{AppHandle, Emitter, State};

use crate::ai::agent;
use crate::ai::provider::{ChatMessage, StreamEvent};
use crate::ai::providers::cli;
use crate::ai::cron::{self, CronContext, CronRunning};
use crate::ai::interaction::{PromptRegistry, SessionState};
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

/// Run one agent turn. Streams events to `ai://chat/<session_id>` and returns the
/// final assistant message for the frontend to append.
#[tauri::command]
pub async fn ai_chat(
    app: AppHandle,
    db: State<'_, Db>,
    home: State<'_, AgentHome>,
    sessions: State<'_, SessionManager>,
    approvals: State<'_, ApprovalRegistry>,
    prompts: State<'_, PromptRegistry>,
    session_state: State<'_, SessionState>,
    llama: State<'_, crate::ai::llama::LlamaServer>,
    edits: State<'_, crate::ai::edits::EditJournal>,
    session_id: String,
    messages: Vec<ChatMessage>,
    provider_id: Option<String>,
    targets: Vec<String>,
    #[allow(non_snake_case)] plan_mode: Option<bool>,
    #[allow(non_snake_case)] workspace_id: Option<String>,
    canvas: Option<Vec<crate::ai::canvas_context::CanvasNode>>,
) -> Result<ChatMessage, String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let event = chat_event(&session_id);
    let app2 = app.clone();
    let forward = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app2.emit(&event, ev);
        }
    });

    let db_inner = db.inner().clone();
    let canvas = canvas.unwrap_or_default();
    // Servers the user has open on the canvas are implicitly actionable: opening a
    // terminal or SFTP panel for a server is a clear signal the agent may work with
    // it. Merge those into the selected targets so the agent can read their
    // scrollback, send commands, and edit files the user is browsing — even if the
    // server wasn't separately ticked in the target picker. (Mutations are still
    // gated by the safety mode.)
    let mut targets = targets;
    for n in &canvas {
        if !n.vps_id.is_empty() && !targets.contains(&n.vps_id) {
            targets.push(n.vps_id.clone());
        }
    }
    let tc = ToolContext {
        app: app.clone(),
        db: db_inner.clone(),
        sessions: sessions.inner().clone(),
        home: home.inner().clone(),
        approvals: approvals.inner().clone(),
        prompts: prompts.inner().clone(),
        session_state: session_state.inner().clone(),
        session_id,
        targets,
        safety: crate::ai::safety::global_safety_mode(&db_inner),
        plan_mode: plan_mode.unwrap_or(false),
        workspace_id: workspace_id.filter(|s| !s.is_empty()),
        canvas,
        edits: edits.inner().clone(),
    };

    // If the chosen provider runs a local server, make sure it's up first so the
    // user doesn't have to start it by hand.
    if let Ok(pid) = crate::ai::registry::active_provider_id(&db_inner, provider_id.as_deref()) {
        if let Ok(Some(p)) = db_inner.get_provider(&pid) {
            if p.kind == "ollama" {
                let base = p.base_url.clone().unwrap_or_else(|| "http://localhost:11434".into());
                if let Err(e) = crate::ai::models::ollama_ensure(&base).await {
                    let _ = tx.send(StreamEvent::Status(format!("Ollama: {e}")));
                }
            } else if p.kind == "llamacpp" {
                let bin_override = p.bin_path.clone().or_else(|| llama_bin_override(&db_inner));
                let status = llama.status(bin_override.as_deref());
                let want = p.model.clone().unwrap_or_default();
                let model_path = if want.trim().is_empty() {
                    String::new()
                } else if std::path::Path::new(&want).is_absolute() {
                    want
                } else {
                    crate::ai::models::models_dir(&app)
                        .join(&want)
                        .to_string_lossy()
                        .into_owned()
                };
                let already = status.running && status.model.as_deref() == Some(model_path.as_str());
                if !already {
                    if model_path.is_empty() {
                        let _ = tx.send(StreamEvent::Status(
                            "llama.cpp: no model selected for this provider — pick a GGUF in Settings → Providers.".into(),
                        ));
                    } else {
                        // Find the binary, or auto-install it the first time (like whisper).
                        let mut bin = crate::ai::llama::find_binary(bin_override.as_deref());
                        if bin.is_none() {
                            let build = db_inner
                                .get_setting("llamacpp.build")
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| "cpu".into());
                            let _ = tx.send(StreamEvent::Status(format!(
                                "Installing llama.cpp ({build} build)…"
                            )));
                            match crate::ai::llama::setup_llama(&app, &build).await {
                                Ok(path) => {
                                    let _ = db_inner.set_setting("llamacpp.bin_path", &path);
                                    let _ = db_inner.set_setting(
                                        "llamacpp.gpu_layers",
                                        if build == "cpu" { "0" } else { "999" },
                                    );
                                    bin = Some(path);
                                }
                                Err(e) => {
                                    let _ = tx.send(StreamEvent::Status(format!(
                                        "llama.cpp install failed: {e}"
                                    )));
                                }
                            }
                        }
                        if let Some(bin) = bin {
                            let port = base_url_port(&p.base_url);
                            let gpu = db_inner
                                .get_setting("llamacpp.gpu_layers")
                                .ok()
                                .flatten()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            let server = (*llama).clone();
                            let _ = tx.send(StreamEvent::Status("Starting llama.cpp server…".into()));
                            if let Err(e) = server.start(&bin, &model_path, port, gpu).await {
                                let _ = tx.send(StreamEvent::Status(format!("llama.cpp: {e}")));
                            }
                        }
                    }
                }
            }
        }
    }

    // Fresh turn — clear any leftover Stop request from a previous turn.
    tc.session_state.clear_cancel(&tc.session_id);
    let result = agent::run_turn(&tc, provider_id.filter(|s| !s.is_empty()), messages, &tx).await;
    // Close the sender and wait for the forwarder to drain, so trailing
    // Activity/ConversationCompacted/Error events are emitted before we return.
    drop(tx);
    let _ = forward.await;
    result
}

/// All file edits the agent made in a chat session (for the changes/diff panel).
#[tauri::command]
pub fn list_file_changes(
    edits: State<'_, crate::ai::edits::EditJournal>,
    session_id: String,
) -> Vec<crate::ai::edits::EditRecord> {
    edits.list(&session_id)
}

/// Forget a session's recorded edits (e.g. when a conversation is deleted).
#[tauri::command]
pub fn clear_file_changes(
    edits: State<'_, crate::ai::edits::EditJournal>,
    session_id: String,
) {
    edits.clear(&session_id);
}

/// Revert one edit by writing its captured "before" content back (or deleting the
/// file if the edit created it). Works for both local and remote files.
#[tauri::command]
pub async fn revert_file_change(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    edits: State<'_, crate::ai::edits::EditJournal>,
    id: String,
) -> Result<(), String> {
    use base64::Engine;
    let rec = edits.get(&id).ok_or("change not found")?;
    if rec.reverted {
        return Err("this change was already reverted".into());
    }
    match rec.scope.as_str() {
        "local" => {
            if rec.is_new {
                std::fs::remove_file(&rec.path).map_err(|e| e.to_string())?;
            } else {
                crate::local::write_local_file(&rec.path, &rec.before).map_err(|e| e.to_string())?;
            }
        }
        "vps" => {
            let vps_id = rec.vps_id.clone().ok_or("change is missing its server id")?;
            let cmd = if rec.is_new {
                format!("rm -f -- {}", crate::ssh::shell_quote(&rec.path))
            } else {
                let b64 = base64::engine::general_purpose::STANDARD.encode(rec.before.as_bytes());
                format!(
                    "printf %s {} | base64 -d > {}",
                    crate::ssh::shell_quote(&b64),
                    crate::ssh::shell_quote(&rec.path)
                )
            };
            sessions
                .run_command(&vps_id, &cmd)
                .await
                .map_err(|e| e.to_string())?;
        }
        other => return Err(format!("unknown change scope '{other}'")),
    }
    edits.mark_reverted(&id);
    let _ = app.emit("agent://file-change-reverted", &id);
    Ok(())
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

/// Resolve a pending command approval (from the UI). When `remember` is set
/// (the "Yes, and don't ask again this chat" choice), the session switches to
/// full autonomy so the rest of the conversation runs without prompts.
#[tauri::command]
pub fn agent_resolve_approval(
    db: State<'_, Db>,
    approvals: State<'_, ApprovalRegistry>,
    session_state: State<'_, SessionState>,
    id: String,
    approved: bool,
    remember: Option<bool>,
    session_id: Option<String>,
) -> Result<(), String> {
    if approved && remember.unwrap_or(false) {
        if let Some(sid) = session_id.as_deref().filter(|s| !s.is_empty()) {
            session_state.set_full_auto(sid);
        }
    }
    let _ = db.resolve_approval(&id, if approved { "approved" } else { "denied" });
    approvals.resolve(&id, approved);
    Ok(())
}

/// Deliver the user's answer to a pending interactive prompt (ask_user) or a
/// plan decision (present_plan: "APPROVE" or "REJECT: <feedback>").
#[tauri::command]
pub fn agent_answer_prompt(
    prompts: State<'_, PromptRegistry>,
    id: String,
    answer: String,
) -> Result<(), String> {
    prompts.resolve(&id, answer);
    Ok(())
}

/// Security-scan a skill at a local path (file or directory) on demand. Uses
/// NVIDIA SkillSpector when installed, else the built-in heuristic scanner.
#[tauri::command]
pub async fn scan_skill_path(path: String) -> Result<crate::ai::skill_scan::ScanReport, String> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("path not found: {path}"));
    }
    Ok(crate::ai::skill_scan::scan_skill(&p).await)
}

// ----- Model discovery / download -----

#[tauri::command]
pub fn get_system_capabilities() -> Result<crate::ai::models::SystemCaps, String> {
    Ok(crate::ai::models::system_capabilities())
}

/// Search installable models. `source` = "ollama" | "huggingface".
#[tauri::command]
pub async fn search_models(
    source: String,
    query: String,
    base_url: Option<String>,
) -> Result<Vec<crate::ai::models::ModelEntry>, String> {
    use crate::ai::models;
    match source.as_str() {
        "huggingface" => models::hf_search(query.trim()).await,
        "ollama" => {
            let base = base_url.unwrap_or_else(|| "http://localhost:11434".into());
            let mut out = models::ollama_list_local(&base).await;
            let installed: std::collections::HashSet<String> =
                out.iter().map(|m| m.id.clone()).collect();
            let q = query.trim().to_lowercase();
            for m in models::ollama_catalog() {
                if installed.contains(&m.id) {
                    continue;
                }
                if q.is_empty() || m.id.to_lowercase().contains(&q) || m.detail.to_lowercase().contains(&q) {
                    out.push(m);
                }
            }
            Ok(out)
        }
        other => Err(format!("unknown model source: {other}")),
    }
}

/// List the GGUF files (with sizes) in a Hugging Face repo.
#[tauri::command]
pub async fn hf_model_files(repo_id: String) -> Result<Vec<crate::ai::models::HfFile>, String> {
    crate::ai::models::hf_files(&repo_id).await
}

/// Download a model. For "huggingface" pass `url` + `filename`; for "ollama"
/// pass `id` (the pull name) + `base_url`. Progress streams on `models://download`.
#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    source: String,
    id: String,
    url: Option<String>,
    filename: Option<String>,
    base_url: Option<String>,
) -> Result<(), String> {
    use crate::ai::models;
    match source.as_str() {
        "huggingface" => {
            let url = url.ok_or("missing url")?;
            let filename = filename.unwrap_or_else(|| {
                url.rsplit('/').next().unwrap_or("model.gguf").to_string()
            });
            models::download_gguf(&app, &id, &url, &filename).await
        }
        "ollama" => {
            let base = base_url.unwrap_or_else(|| "http://localhost:11434".into());
            models::ollama_pull(&app, &base, &id).await
        }
        other => Err(format!("unknown model source: {other}")),
    }
}

/// Downloaded GGUF files on this machine.
#[tauri::command]
pub fn list_local_files(app: AppHandle) -> Result<Vec<crate::ai::models::LocalFile>, String> {
    Ok(crate::ai::models::list_local_gguf(&app))
}

// ----- Managed llama.cpp server -----

fn llama_bin_override(db: &Db) -> Option<String> {
    db.get_setting("llamacpp.bin_path")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
}

/// Extract the port from a base URL like `http://127.0.0.1:8080/v1` (defaults 8080).
fn base_url_port(base: &Option<String>) -> u16 {
    base.as_deref()
        .and_then(|u| u.rsplit(':').next())
        .and_then(|tail| tail.split('/').next())
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(8080)
}

#[tauri::command]
pub fn llama_server_status(
    state: State<'_, crate::ai::llama::LlamaServer>,
    db: State<'_, Db>,
) -> Result<crate::ai::llama::LlamaStatus, String> {
    Ok(state.status(llama_bin_override(&db).as_deref()))
}

/// Start the managed llama-server for a downloaded GGUF on `port`.
#[tauri::command]
pub async fn llama_server_start(
    app: AppHandle,
    state: State<'_, crate::ai::llama::LlamaServer>,
    db: State<'_, Db>,
    model_file: String,
    port: u16,
    gpu_layers: Option<u32>,
) -> Result<(), String> {
    let bin = crate::ai::llama::find_binary(llama_bin_override(&db).as_deref()).ok_or(
        "llama-server not found. Install llama.cpp so `llama-server` is on your PATH, \
         or set its full path in Settings (llamacpp.bin_path).",
    )?;
    let path = crate::ai::models::models_dir(&app)
        .join(&model_file)
        .to_string_lossy()
        .to_string();
    // Clone the (Arc-backed) handle so we don't hold the State guard across await.
    let server = (*state).clone();
    server.start(&bin, &path, port, gpu_layers.unwrap_or(0)).await
}

#[tauri::command]
pub fn llama_server_stop(state: State<'_, crate::ai::llama::LlamaServer>) -> Result<(), String> {
    state.stop();
    Ok(())
}

// ----- Ollama daemon management -----

#[tauri::command]
pub async fn ollama_status(base_url: Option<String>) -> Result<crate::ai::models::OllamaStatus, String> {
    let base = base_url.unwrap_or_else(|| "http://localhost:11434".into());
    let bin = crate::ai::models::find_ollama_binary();
    let running = crate::ai::models::ollama_running(&base).await;
    Ok(crate::ai::models::OllamaStatus {
        installed: bin.is_some(),
        running,
        bin,
    })
}

#[tauri::command]
pub async fn ollama_ensure(base_url: Option<String>) -> Result<bool, String> {
    let base = base_url.unwrap_or_else(|| "http://localhost:11434".into());
    crate::ai::models::ollama_ensure(&base).await
}

// ----- Voice: speech-to-text -----

/// Transcribe base64 WAV audio. `engine` = "local" (whisper-cli) | "cloud".
#[tauri::command]
pub async fn transcribe(
    app: AppHandle,
    db: State<'_, Db>,
    audio_b64: String,
    engine: String,
    model_file: Option<String>,
    lang: Option<String>,
) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_b64.as_bytes())
        .map_err(|e| format!("invalid audio: {e}"))?;
    let db = db.inner().clone();
    let lang = lang.unwrap_or_else(|| "auto".into());
    match engine.as_str() {
        "cloud" => crate::ai::voice::transcribe_cloud(&db, &bytes, &lang).await,
        "groq" => crate::ai::voice::transcribe_groq(&db, &bytes, &lang).await,
        "parakeet" => crate::ai::parakeet::transcribe_parakeet(&app, &db, &bytes, &lang).await,
        _ => {
            crate::ai::voice::transcribe_local(
                &app,
                &db,
                &bytes,
                model_file.as_deref().unwrap_or(""),
                &lang,
            )
            .await
        }
    }
}

/// Download a whisper GGML model by filename (multilingual or English). Returns the filename.
#[tauri::command]
pub async fn download_whisper_model(app: AppHandle, model_file: String) -> Result<String, String> {
    crate::ai::voice::download_whisper_model(&app, &model_file).await
}

/// Install GPU (Vulkan) Parakeet STT + model. Progress on `voice://parakeet-setup`.
#[tauri::command]
pub async fn setup_parakeet(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let bin = crate::ai::parakeet::setup_parakeet(&app).await?;
    db.set_setting("parakeet.bin_path", &bin)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Install a prebuilt llama-server (Windows) and store its path. Returns the path.
#[tauri::command]
pub async fn setup_llama(app: AppHandle, db: State<'_, Db>) -> Result<String, String> {
    let build = db
        .get_setting("llamacpp.build")
        .ok()
        .flatten()
        .unwrap_or_else(|| "cpu".into());
    let path = crate::ai::llama::setup_llama(&app, &build).await?;
    db.set_setting("llamacpp.bin_path", &path)
        .map_err(|e| e.to_string())?;
    // GPU builds: offload all layers by default; CPU: none.
    let _ = db.set_setting("llamacpp.gpu_layers", if build == "cpu" { "0" } else { "999" });
    Ok(path)
}

/// Text-to-speech. `engine` = "piper" (offline neural) | "cloud" (OpenAI). "os" is
/// handled entirely in the webview. Returns base64 WAV.
#[tauri::command]
pub async fn synthesize(
    app: AppHandle,
    db: State<'_, Db>,
    text: String,
    voice: Option<String>,
    engine: Option<String>,
    instructions: Option<String>,
) -> Result<String, String> {
    use base64::Engine as _;
    let db = db.inner().clone();
    let v = voice.unwrap_or_default();
    let instr = instructions.unwrap_or_default();
    let bytes = match engine.as_deref() {
        Some("piper") => crate::ai::piper::synthesize_local_piper(&app, &db, &text, &v).await?,
        Some("edge") => crate::ai::edge_tts::synthesize_edge(&app, &text, &v).await?,
        _ => crate::ai::voice::synthesize_cloud(&db, &text, &v, &instr).await?,
    };
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Set up the free Edge TTS voice (isolated Python venv + edge-tts). Progress on `voice://edge-setup`.
#[tauri::command]
pub async fn setup_edge_tts(app: AppHandle) -> Result<(), String> {
    crate::ai::edge_tts::setup_edge(&app).await
}

/// Stop the running agent turn for a session (user pressed Stop). The turn loop
/// halts at its next checkpoint.
#[tauri::command]
pub fn agent_cancel(
    session_state: State<'_, SessionState>,
    session_id: String,
) -> Result<(), String> {
    session_state.cancel(&session_id);
    Ok(())
}

/// Install the offline Piper voice engine + its default voice. Progress on `voice://piper-setup`.
#[tauri::command]
pub async fn setup_piper(app: AppHandle, db: State<'_, Db>) -> Result<String, String> {
    let bin = crate::ai::piper::setup_piper(&app).await?;
    db.set_setting("piper.bin_path", &bin).map_err(|e| e.to_string())?;
    Ok(crate::ai::piper::DEFAULT_VOICE.to_string())
}

/// Download an additional Piper voice (e.g. ro_RO-mihai-medium). Returns the voice key.
#[tauri::command]
pub async fn download_piper_voice(app: AppHandle, voice: String) -> Result<String, String> {
    crate::ai::piper::download_voice(&app, &voice).await
}

/// One-click local-voice setup: download whisper.cpp + a base model, wire the
/// binary path. Returns the installed model filename. Progress on `voice://whisper-setup`.
#[tauri::command]
pub async fn setup_whisper(app: AppHandle, db: State<'_, Db>) -> Result<String, String> {
    let db = db.inner().clone();
    let bin = crate::ai::voice::setup_whisper(&app).await?;
    db.set_setting("whisper.bin_path", &bin).map_err(|e| e.to_string())?;
    Ok(crate::ai::voice::default_whisper_model().to_string())
}

/// Remove a model: an installed Ollama model (`source="ollama"`, `id`=name) or a
/// downloaded GGUF file (`source="gguf"`, `id`=filename).
#[tauri::command]
pub async fn delete_model(
    app: AppHandle,
    source: String,
    id: String,
    base_url: Option<String>,
) -> Result<(), String> {
    use crate::ai::models;
    match source.as_str() {
        "ollama" => {
            let base = base_url.unwrap_or_else(|| "http://localhost:11434".into());
            models::delete_ollama(&base, &id).await
        }
        "gguf" | "huggingface" => models::delete_local_gguf(&app, &id),
        other => Err(format!("unknown model source: {other}")),
    }
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
    if !cron::schedule_is_valid(&input.schedule) {
        return Err(format!(
            "invalid schedule '{}'; use e.g. '@every 5m', '@hourly', '@daily 09:30', '@weekly mon 08:00'",
            input.schedule
        ));
    }
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
        .get_cron_job(&id)
        .map_err(|e| e.to_string())?
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
    let forward = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app2.emit(&event, ev);
        }
    });

    let result = cli::login(&provider.kind, &bin, Some(&tx)).await;
    drop(tx);
    let _ = forward.await;
    result
}
