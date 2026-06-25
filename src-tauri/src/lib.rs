mod ai;
/// Headless benchmark / eval harness (driven by the `xconsole-bench` bin).
pub mod bench;
mod commands;
/// At-rest encryption primitives (AES-256-GCM + PBKDF2) for DB encryption.
pub mod crypto;
/// The `db.lock.json` manifest (salt + wrapped data key) for the app lock.
pub mod lock;
mod infra;
mod local;
pub mod mcp;
mod proc;
mod secrets;
mod ssh;
mod storage;

use ai::interaction::{PromptRegistry, SessionState};
use ai::safety::ApprovalRegistry;
use ai::AgentHome;
use ssh::{SessionManager, SftpManager};
use storage::Db;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Restore the window to where it was last left (position/size/maximized),
        // on whichever monitor it was on. Minimized isn't restored — see the guard
        // in setup() that centers an off-screen window instead.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .setup(|app| {
            // Local database under the app data dir.
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"));
            let db_path = dir.join("xconsole.db");

            // Safety net: if the app version changed since last run (i.e. just after
            // an auto-update), snapshot the DB BEFORE any schema migration touches it,
            // so a bad migration can never cost the user their chats / workspaces /
            // settings. Kept as a single rolling backup next to the DB.
            let version_marker = dir.join("app_version.txt");
            let current_version = app.package_info().version.to_string();
            let enc_for_bak = dir.join("xconsole.db.enc");
            if db_path.exists() || enc_for_bak.exists() {
                let last = std::fs::read_to_string(&version_marker).unwrap_or_default();
                if !last.trim().is_empty() && last.trim() != current_version {
                    // Back up the encrypted blob if the lock is on, else the plaintext DB.
                    let (src, backup) = if enc_for_bak.exists() {
                        (enc_for_bak.clone(), dir.join("xconsole.db.enc.bak"))
                    } else {
                        (db_path.clone(), dir.join("xconsole.db.bak"))
                    };
                    if let Err(e) = std::fs::copy(&src, &backup) {
                        eprintln!("xconsole: pre-update DB backup failed: {e}");
                    } else {
                        eprintln!(
                            "xconsole: backed up DB before update ({} -> {})",
                            last.trim(),
                            current_version
                        );
                    }
                }
            }

            // At-rest encryption (Approach B): if the app lock is configured, open the
            // encrypted DB using the remembered device key; if there's no remembered key, start
            // a LOCKED PLACEHOLDER so the frontend can show the unlock screen (the unlock
            // command swaps the real connection in). With no lock, open the plaintext DB as before.
            let enc_path = dir.join("xconsole.db.enc");
            let mut initial_data_key: Option<[u8; crate::crypto::KEY_LEN]> = None;
            let db = if crate::lock::is_lock_enabled(&dir) {
                match crate::secrets::get_data_key().ok().flatten() {
                    Some(key) => match Db::open_encrypted(&enc_path, &db_path, &dir, &key) {
                        Ok(db) => {
                            initial_data_key = Some(key);
                            db
                        }
                        Err(e) => {
                            eprintln!("xconsole: silent unlock failed ({e}); showing unlock screen");
                            Db::open_locked().expect("failed to open placeholder db")
                        }
                    },
                    None => Db::open_locked().expect("failed to open placeholder db"),
                }
            } else {
                Db::open(&db_path).expect("failed to open database")
            };
            // Record the version we successfully opened at, for the next launch's check.
            let _ = std::fs::write(&version_marker, &current_version);

            let handle = app.handle().clone();
            let sessions = SessionManager::new(handle, db.clone());
            let sftp = SftpManager::new(db.clone());

            // Agent home: editable Hermes-format files (SOUL.md / MEMORY.md / ...).
            let agent_home = AgentHome::new(dir.join("agent"));
            ai::skills::seed_defaults(&agent_home);

            let approvals = ApprovalRegistry::new();
            let prompts = PromptRegistry::new();
            let session_state = SessionState::new();
            let llama_server = ai::llama::LlamaServer::default();
            let cron_running = ai::cron::CronRunning::default();

            // Background cron scheduler. Reuses the same exec/agent/safety paths.
            ai::cron::spawn(ai::cron::CronContext {
                app: app.handle().clone(),
                db: db.clone(),
                sessions: sessions.clone(),
                home: agent_home.clone(),
                approvals: approvals.clone(),
                running: cron_running.clone(),
            });

            app.manage(commands::lock::DataKey(std::sync::Mutex::new(initial_data_key)));
            app.manage(db);
            app.manage(sessions);
            app.manage(sftp);
            app.manage(agent_home);
            app.manage(approvals);
            app.manage(prompts);
            app.manage(session_state);
            app.manage(llama_server);
            app.manage(cron_running);
            app.manage(ai::edits::EditJournal::new());

            // The Cursor MCP runs as a SEPARATE process (Cursor spawns it) and can't
            // emit Tauri events, so its canvas tools drop request files in this shared
            // queue dir. Watch it and forward each request to the live canvas.
            {
                let app_handle = app.handle().clone();
                let queue_dir = dir.join("canvas-queue");
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        let rd = match std::fs::read_dir(&queue_dir) {
                            Ok(r) => r,
                            Err(_) => continue, // dir doesn't exist yet — nothing queued
                        };
                        let mut paths: Vec<std::path::PathBuf> = rd
                            .flatten()
                            .map(|e| e.path())
                            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
                            .collect();
                        paths.sort();
                        for path in paths {
                            if let Ok(bytes) = std::fs::read(&path) {
                                if let Ok(payload) =
                                    serde_json::from_slice::<serde_json::Value>(&bytes)
                                {
                                    let _ = app_handle.emit("canvas://command", payload);
                                }
                            }
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                });
            }

            // The window-state plugin has already restored position/size/maximized.
            // If the saved position is off all monitors (e.g. it was minimized at
            // close → Windows stores -32000), center it on the current monitor.
            // Either way, open un-minimized and focused.
            if let Some(win) = app.get_webview_window("main") {
                let on_screen = win
                    .outer_position()
                    .ok()
                    .zip(win.available_monitors().ok())
                    .map(|(pos, mons)| {
                        mons.iter().any(|m| {
                            let mp = m.position();
                            let ms = m.size();
                            pos.x >= mp.x - 64
                                && pos.y >= mp.y - 64
                                && pos.x < mp.x + ms.width as i32
                                && pos.y < mp.y + ms.height as i32
                        })
                    })
                    .unwrap_or(false);
                if !on_screen {
                    let _ = win.center();
                }
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vps::list_vps,
            commands::vps::save_vps,
            commands::vps::delete_vps,
            commands::vps::reorder_vps,
            commands::vps::setup_vps_key_auth,
            commands::session::ssh_connect,
            commands::session::ssh_write,
            commands::session::ssh_resize,
            commands::session::ssh_disconnect,
            commands::session::ssh_replay,
            commands::sftp::sftp_connect,
            commands::sftp::sftp_list,
            commands::sftp::sftp_download,
            commands::sftp::sftp_write,
            commands::sftp::sftp_disconnect,
            commands::remote_file::vps_file_stat,
            commands::remote_file::vps_file_chmod,
            commands::remote_file::vps_file_chown,
            commands::remote_file::vps_file_delete,
            commands::remote_file::vps_file_rename,
            commands::remote_file::vps_file_mkdir,
            commands::remote_file::vps_file_touch,
            commands::workspace::list_workspaces,
            commands::workspace::save_workspace,
            commands::workspace::delete_workspace,
            commands::workspace::get_workspace_brief,
            commands::workspace::save_workspace_brief,
            commands::workspace::list_known_hosts,
            commands::workspace::forget_host_key,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::list_settings,
            commands::settings::delete_setting,
            commands::settings::list_providers,
            commands::settings::save_provider,
            commands::settings::delete_provider,
            commands::update::check_for_update,
            commands::update::start_app_update,
            commands::ai::ai_cli_login,
            commands::ai::ai_chat,
            commands::ai::list_agent_conversations,
            commands::ai::get_agent_conversation,
            commands::ai::save_agent_conversation,
            commands::ai::delete_agent_conversation,
            commands::ai::agent_resolve_approval,
            commands::ai::agent_answer_prompt,
            commands::ai::agent_cancel,
            commands::ai::list_file_changes,
            commands::ai::clear_file_changes,
            commands::ai::revert_file_change,
            commands::ai::scan_skill_path,
            commands::ai::get_system_capabilities,
            commands::ai::search_models,
            commands::ai::hf_model_files,
            commands::ai::download_model,
            commands::ai::list_local_files,
            commands::ai::delete_model,
            commands::ai::llama_server_status,
            commands::ai::llama_server_start,
            commands::ai::llama_server_stop,
            commands::ai::ollama_status,
            commands::ai::ollama_ensure,
            commands::ai::transcribe,
            commands::ai::synthesize,
            commands::ai::setup_whisper,
            commands::ai::download_whisper_model,
            commands::ai::setup_piper,
            commands::ai::download_piper_voice,
            commands::ai::setup_edge_tts,
            commands::ai::setup_parakeet,
            commands::ai::setup_llama,
            commands::ai::list_pending_approvals,
            commands::ai::get_agent_docs,
            commands::ai::save_soul,
            commands::ai::save_memory_doc,
            commands::ai::save_user_doc,
            commands::ai::list_skills,
            commands::ai::get_skill,
            commands::ai::save_skill,
            commands::ai::delete_skill,
            commands::ai::list_cron_jobs,
            commands::ai::save_cron_job,
            commands::ai::delete_cron_job,
            commands::ai::run_cron_job,
            commands::infra::list_infra_projects,
            commands::infra::save_infra_project,
            commands::infra::delete_infra_project,
            commands::infra::get_infra_project,
            commands::infra::read_project_file_cmd,
            commands::cloud::list_cloud_accounts,
            commands::cloud::save_cloud_account,
            commands::cloud::delete_cloud_account,
            commands::cloud::list_tfc_workspaces,
            commands::cloud::list_cloud_resources,
            commands::lock::lock_status,
            commands::lock::setup_lock,
            commands::lock::unlock_with_password,
            commands::lock::change_password,
            commands::lock::forget_device,
            commands::lock::disable_lock,
            commands::lock::export_unencrypted_backup,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // On exit: final encrypted persist so a crash/last-second write can't be lost, then
            // the next launch removes the plaintext working file (see Db::open_encrypted).
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                if let Some(db) = app.try_state::<Db>() {
                    db.finalize_on_exit();
                }
            }
        });
}
