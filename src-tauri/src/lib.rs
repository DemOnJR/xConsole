mod ai;
/// Headless benchmark / eval harness (driven by the `xconsole-bench` bin).
pub mod bench;
mod commands;
/// MySQL/MariaDB client: discovery, SSH-tunnelled connections, browsing.
mod db;
/// At-rest encryption primitives (AES-256-GCM + PBKDF2) for DB encryption.
pub mod crypto;
/// The `db.lock.json` manifest (salt + wrapped data key) for the app lock.
pub mod lock;
mod infra;
mod artifacts;
mod autostart;
mod local;
pub mod mcp;
pub mod plugins;
mod proc;
mod secrets;
mod ssh;
mod storage;

use ai::interaction::{PromptRegistry, SessionState};
use ai::safety::ApprovalRegistry;
use ai::AgentHome;
use ssh::transfer::TransferManager;
use ssh::{SessionManager, SftpManager};
use storage::Db;
use tauri::{Emitter, Manager};

/// Append a line to `xconsole.log` in the app data dir.
///
/// A release build sets `windows_subsystem = "windows"`, so it has no console and every
/// `eprintln!` goes nowhere — which is why an app that exited on its own left no trace at
/// all, in the event log or on stderr. Diagnosing anything about startup or shutdown needs
/// a file.
pub(crate) fn diag(msg: &str) {
    let Some(dir) = dirs_next_app_data() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("xconsole.log");
    // Keep it from growing without bound; this is a rolling breadcrumb trail, not an audit
    // log, and it must never be the reason the disk fills up.
    if std::fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    }
}

/// Append one JSON line to a file in the app data dir (prompt-cache traces, etc.).
///
/// Release builds have no console, so `eprintln!` never reaches the user. Structured
/// lines here survive so we can see hit/miss after a real session.
pub(crate) fn diag_jsonl(filename: &str, json_line: &str) {
    let Some(dir) = dirs_next_app_data() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(filename);
    // Rotate at 2 MiB — a long weekend of agent turns, not an audit archive.
    if std::fs::metadata(&path).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{json_line}");
    }
}

/// The app data dir, resolved without an AppHandle so the panic hook can use it.
pub(crate) fn dirs_next_app_data() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|p| std::path::PathBuf::from(p).join("com.xconsole.app"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Record panics before anything else can swallow them. A panic on a background thread
    // does not necessarily kill the process, so these would otherwise vanish silently.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        diag(&format!(
            "PANIC: {info}\nbacktrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
        previous(info);
    }));
    diag(&format!("--- start (pid {}) ---", std::process::id()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        // Terminal copy/paste. Needs a real clipboard, not navigator.clipboard: the
        // webview denies programmatic reads (Ctrl+right-click paste) and cannot see
        // image data at all, which is how screenshots reach a terminal.
        .plugin(tauri_plugin_clipboard_manager::init())
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
                            // Keychain secrets are encrypted with this same key, so
                            // install it before anything tries to read one.
                            crate::secrets::set_wrapping_key(Some(key));
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

            // Encrypt any keychain secret still stored in the clear — but ONLY if the
            // user has explicitly opted in.
            //
            // Wrapping is a one-way door for older builds: they don't know the `xcw1:`
            // tag, so they hand the ciphertext to the SSH server as the password and
            // every connection fails with "authentication failed". Doing that silently
            // at startup means a single launch of a newer build — a dev build, a test,
            // a rollback that got reverted — permanently breaks whatever build the user
            // actually runs day to day, with no clue as to why.
            //
            // So the conversion is a deliberate action (Settings → Security), and this
            // only catches up a user who already opted in on a configuration that never
            // passes through unlock: lock enabled plus device remembered unlocks
            // silently right here.
            //
            // The wrapping key is installed unconditionally above regardless, because
            // it is needed to READ secrets that are already wrapped.
            if let Some(key) = initial_data_key {
                let db_for_migration = db.clone();
                tauri::async_runtime::spawn(async move {
                    if !crate::secrets::encryption_opted_in(&db_for_migration) {
                        return;
                    }
                    let keys = crate::secrets::all_secret_keys(&db_for_migration);
                    if !keys.is_empty() {
                        crate::secrets::rekey_all(&keys, Some(key));
                    }
                });
            }

            let handle = app.handle().clone();
            let sessions = SessionManager::new(handle, db.clone());
            let sftp = SftpManager::new(db.clone());

            // Agent home: editable Hermes-format files (SOUL.md / MEMORY.md / ...).
            let agent_home = AgentHome::new(dir.join("agent"));
            ai::skills::seed_defaults(&agent_home);
            // One-time: fold legacy USER.md into the consolidated TASTE.md store.
            ai::memory::migrate_user_into_taste(&agent_home);

            let approvals = ApprovalRegistry::new();
            let prompts = PromptRegistry::new();
            let session_state = SessionState::new();
            let llama_server = ai::llama::LlamaServer::default();
            let cron_running = ai::cron::CronRunning::default();
            let goal_running = ai::goal::GoalRunning::default();

            // Background cron scheduler. Reuses the same exec/agent/safety paths.
            ai::cron::spawn(ai::cron::CronContext {
                app: app.handle().clone(),
                db: db.clone(),
                sessions: sessions.clone(),
                home: agent_home.clone(),
                approvals: approvals.clone(),
                running: cron_running.clone(),
            });

            // Goal ticker: restarts orphaned "active" loops, wakes waiting sessions
            // when due, and hands standing work to idle personas so they do not sit.
            ai::goal::spawn_tick(ai::goal::GoalContext {
                app: app.handle().clone(),
                db: db.clone(),
                sessions: sessions.clone(),
                home: agent_home.clone(),
                approvals: approvals.clone(),
                running: goal_running.clone(),
                session_state: session_state.clone(),
            });

            app.manage(commands::lock::DataKey(std::sync::Mutex::new(initial_data_key)));
            app.manage(commands::lock::AutoLock::default());
            app.manage(ai::edits::EditJournal::with_db(db.clone()));
            app.manage(db);
            app.manage(sessions);
            app.manage(sftp);
            app.manage(TransferManager::new());
            app.manage(commands::db::DbSessions::new());
            // Lifecycle hooks: snapshot hooks.json at startup so a
            // mid-session edit (incl. one the agent might write) only takes effect on
            // an explicit reload. Loaded before agent_home is moved into managed state.
            app.manage(ai::hooks::HooksState::new(ai::hooks::HooksConfig::load(
                &agent_home,
            )));
            app.manage(agent_home);
            app.manage(approvals);
            app.manage(prompts);
            app.manage(session_state);
            app.manage(llama_server);
            app.manage(cron_running);
            app.manage(goal_running);

            // Keep the Run key pointing at this build if the user opted in.
            crate::autostart::refresh_if_enabled();

            // Idle auto-lock. The timer lives in the backend on purpose: a JS timer stops
            // with a hung or crashed webview, and "the lock quietly stopped working" is the
            // failure mode you never notice. The frontend only reports activity.
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                        let db = app_handle.state::<storage::Db>();
                        // Nothing to lock unless a lock is configured AND we are unlocked.
                        if !db.is_encrypted() {
                            continue;
                        }
                        let Some(timeout) = commands::lock::auto_lock_timeout(&db) else {
                            continue;
                        };
                        let idle = app_handle.state::<commands::lock::AutoLock>();
                        // No heartbeat yet => we cannot tell idle from busy. Wait rather
                        // than lock on a blind schedule.
                        if !idle.can_measure_idleness() || idle.idle_for() < timeout {
                            continue;
                        }
                        let datakey = app_handle.state::<commands::lock::DataKey>();
                        let closed = commands::lock::close_everything(
                            &app_handle.state::<SessionManager>(),
                            &app_handle.state::<SftpManager>(),
                            &app_handle.state::<commands::db::DbSessions>(),
                        );
                        let saved = db.relock();
                        crate::secrets::set_wrapping_key(None);
                        if let Some(mut k) = datakey.0.lock().unwrap().take() {
                            use zeroize::Zeroize;
                            k.zeroize();
                        }
                        if let Err(e) = saved {
                            eprintln!("xconsole: auto-lock saved nothing: {e}");
                        }
                        let _ = app_handle.emit("app://locked", closed as u64);
                    }
                });
            }

            // Remote control (Discord). Polls outbound only when the user has enabled
            // and configured it, so this costs nothing otherwise — and xConsole never
            // opens a port, which is the security promise the README makes.
            ai::remote::spawn(app.handle().clone());

            // The other direction. A transport driver only ever writes the reply to the
            // message it is answering, so work that finishes minutes or hours later had
            // no way back to the person who asked. This drains the durable queue that
            // gives it one.
            ai::remote::outbox::spawn(app.handle().clone());

            // The Cursor MCP runs as a SEPARATE process (Cursor spawns it) and can't
            // emit Tauri events, so its canvas tools drop request files in this shared
            // queue dir. Watch it and forward each request to the live canvas.
            {
                let app_handle = app.handle().clone();
                let queue_dir = dir.join("canvas-queue");
                tauri::async_runtime::spawn(async move {
                    // Adaptive backoff. Most users never run the Cursor MCP, so this
                    // directory usually does not exist — a flat 500 ms poll was a
                    // `read_dir` syscall twice a second, forever, for nothing.
                    //
                    // Poll quickly while requests are arriving (a burst of canvas
                    // commands is one interaction, so the follow-ups are already
                    // queued), then back off toward a slow idle tick. First request
                    // after an idle stretch waits up to IDLE_MAX; after that the queue
                    // drains at FAST, which is quicker than the old fixed interval.
                    const FAST: std::time::Duration = std::time::Duration::from_millis(250);
                    const IDLE_MAX: std::time::Duration = std::time::Duration::from_secs(5);
                    let mut delay = FAST;
                    loop {
                        tokio::time::sleep(delay).await;
                        let Ok(rd) = std::fs::read_dir(&queue_dir) else {
                            // Directory doesn't exist yet — nothing is queued and
                            // nothing will be until an MCP client creates it.
                            delay = IDLE_MAX;
                            continue;
                        };
                        let mut paths: Vec<std::path::PathBuf> = rd
                            .flatten()
                            .map(|e| e.path())
                            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
                            .collect();
                        if paths.is_empty() {
                            delay = (delay * 2).min(IDLE_MAX);
                            continue;
                        }
                        delay = FAST;
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
            commands::artifacts::list_artifacts,
            commands::artifacts::verify_artifact,
            commands::artifacts::reveal_artifact,
            commands::artifacts::delete_artifact,
            commands::artifacts::artifacts_dir,
            commands::session::ssh_connect,
            commands::session::remote_git_branch,
            commands::session::ssh_write,
            commands::session::ssh_resize,
            commands::session::ssh_disconnect,
            commands::session::ssh_replay,
            commands::sftp::sftp_connect,
            commands::sftp::sftp_list,
            commands::sftp::sftp_download,
            commands::sftp::sftp_write,
            commands::sftp::sftp_mkdir,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_remove,
            commands::sftp::sftp_symlink,
            commands::sftp::sftp_disconnect,
            commands::sftp::pick_directory,
            commands::sftp::pick_files,
            commands::sftp::pick_file,
            commands::sftp::local_fs_list,
            commands::sftp::local_fs_home,
            commands::sftp::local_fs_read_text,
            commands::sftp::local_fs_read_bytes,
            commands::sftp::local_git_branch,
            commands::sftp::sftp_transfer_start,
            commands::sftp::sftp_archive_start,
            commands::sftp::sftp_transfer_cancel,
            commands::sftp::sftp_transfer_list,
            commands::sftp::sftp_transfer_clear_finished,
            commands::sftp::sftp_edit_external,
            commands::remote_file::vps_file_stat,
            commands::remote_file::vps_file_chmod,
            commands::remote_file::vps_file_chown,
            commands::remote_file::vps_file_delete,
            commands::remote_file::vps_file_rename,
            commands::remote_file::vps_file_symlink,
            commands::remote_file::vps_file_copy,
            commands::remote_file::vps_file_delete_many,
            commands::remote_file::vps_file_search,
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
            commands::settings::get_autostart,
            commands::settings::set_autostart,
            commands::update::check_for_update,
            commands::update::start_app_update,
            commands::update::get_update_channel,
            commands::update::set_update_channel,
            commands::ai::ai_cli_login,
            commands::ai::ai_cli_models,
            commands::ai::ai_provider_models,
            commands::ai::ai_list_models,
            commands::ai::ai_sync_prices,
            commands::ai::ai_get_model_prices,
            commands::ai::ai_set_model_price,
            commands::ai::ai_chat,
            commands::ai::list_agent_conversations,
            commands::ai::agent_analytics,
            commands::ai::app_resource_snapshot,
            commands::ai::get_agent_conversation,
            commands::ai::save_agent_conversation,
            commands::ai::delete_agent_conversation,
            commands::ai::agent_resolve_approval,
            commands::ai::agent_answer_prompt,
            commands::ai::agent_cancel,
            commands::ai::list_file_changes,
            commands::ai::list_file_changes_history,
            commands::ai::clear_file_changes,
            commands::ai::revert_file_change,
            commands::ai::list_plans,
            commands::ai::get_plan,
            commands::ai::archive_plan,
            commands::ai::cancel_plan,
            commands::ai::scan_skill_path,
            commands::ai::skill_scanner_status,
            commands::ai::install_skill_scanner,
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
            commands::ai::save_taste_doc,
            commands::ai::save_soul,
            commands::ai::save_memory_doc,
            commands::ai::get_hooks_config,
            commands::ai::save_hooks_config,
            commands::ai::reload_hooks,
            commands::ai::hooks_status,
            commands::ai::list_skills,
            commands::ai::get_skill,
            commands::ai::save_skill,
            commands::ai::delete_skill,
            commands::ai::list_cron_jobs,
            commands::ai::save_cron_job,
            commands::ai::delete_cron_job,
            commands::ai::run_cron_job,
            commands::remote::get_remote_status,
            commands::remote::save_remote_config,
            commands::remote::clear_remote_token,
            commands::remote::reset_remote_conversation,
            commands::remote::test_remote_token,
            commands::remote::whatsapp_status,
            commands::remote::whatsapp_link_start,
            commands::remote::whatsapp_link_cancel,
            commands::remote::whatsapp_unlink,
            commands::remote::whatsapp_chats,
            commands::remote::whatsapp_rebuild_helper,
            commands::remote::whatsapp_auto_install,
            commands::persona::list_personas,
            commands::persona::save_persona,
            commands::persona::delete_persona,
            commands::persona::persona_org_chart,
            commands::persona::list_agent_messages,
            commands::project::project_history,
            commands::persona::agent_activity,
            commands::persona::create_team,
            commands::project::project_metrics,
            commands::persona::unread_user_messages,
            commands::persona::mark_agent_messages_read,
            commands::persona::post_agent_message,
            commands::goal::start_goal,
            commands::goal::confirm_goal,
            commands::goal::pause_goal,
            commands::goal::continue_goal,
            commands::goal::stop_goal,
            commands::goal::get_goal,
            commands::goal::list_goals,
            commands::goal::delete_goal,
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
            commands::cloudflare::start_cloudflare_oauth_login,
            commands::cloudflare::save_cloudflare_manual_token,
            commands::cloudflare::list_cloudflare_zones,
            commands::cloudflare::list_cloudflare_tunnels,
            commands::cloudflare::create_cloudflare_tunnel,
            commands::cloudflare::delete_cloudflare_tunnel,
            commands::cloudflare::get_cloudflare_tunnel_config,
            commands::cloudflare::save_cloudflare_tunnel_config,
            commands::cloudflare::get_cloudflare_tunnel_token,
            commands::cloudflare::list_cloudflare_dns_records,
            commands::cloudflare::upsert_cloudflare_dns_record,
            commands::cloudflare::delete_cloudflare_dns_record,
            commands::cloudflare::get_cloudflare_security_settings,
            commands::cloudflare::set_cloudflare_security_level,
            commands::cloudflare::list_cloudflare_history,
            commands::cloudflare::revert_cloudflare_action,
            commands::cloudflare::get_cloudflare_zone_analytics,
            commands::plugin::list_installed_plugins,
            commands::plugin::get_disabled_plugin_ids_cmd,
            commands::plugin::get_plugin_readme_cmd,
            commands::plugin::install_plugin_cmd,
            commands::plugin::link_plugin_cmd,
            commands::plugin::uninstall_plugin_cmd,
            commands::plugin::toggle_plugin_cmd,
            commands::plugin::reload_plugins_cmd,
            commands::plugin::check_plugin_updates_cmd,
            commands::plugin::check_single_plugin_update_cmd,
            commands::plugin::update_plugin_cmd,
            commands::plugin::update_all_plugins_cmd,
            commands::plugin::set_plugin_remote_cmd,
            commands::db::db_discover,
            commands::db::db_connect,
            commands::db::db_disconnect,
            commands::db::db_save_connection,
            commands::db::db_list_connections,
            commands::db::db_forget_connection,
            commands::db::db_connect_saved,
            commands::db::db_use_database,
            commands::db::db_list_databases,
            commands::db::db_list_tables,
            commands::db::db_describe_table,
            commands::db::db_select_page,
            commands::db::db_run_sql,
            commands::db::db_update_cell,
            commands::db::db_delete_row,
            commands::db::db_delete_rows,
            commands::lock::lock_status,
            commands::lock::setup_lock,
            commands::lock::unlock_with_password,
            commands::lock::change_password,
            commands::lock::forget_device,
            commands::lock::set_secret_encryption,
            commands::lock::disable_lock,
            commands::lock::export_unencrypted_backup,
            commands::upload::terminal_upload,
            commands::settings::log_diag,
            commands::lock::lock_now,
            commands::lock::note_activity,
            commands::lock::get_auto_lock_minutes,
            commands::lock::set_auto_lock_minutes,
            commands::teams::list_channel_messages,
            commands::teams::list_channel_thread,
            commands::teams::post_channel_message,
            commands::teams::list_agent_log,
            commands::teams::channel_unread,
            commands::teams::mark_channel_read,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // On exit: final encrypted persist so a crash/last-second write can't be lost, then
            // the next launch removes the plaintext working file (see Db::open_encrypted).
            // Log every shutdown-shaped event with its cause. "The app closes randomly"
            // is unanswerable without knowing whether a window asked to close, the
            // runtime decided to exit, or the process simply vanished.
            match &event {
                tauri::RunEvent::WindowEvent { label, event: we, .. } => {
                    if matches!(we, tauri::WindowEvent::CloseRequested { .. }) {
                        diag(&format!("WindowEvent::CloseRequested on '{label}'"));
                    }
                    if matches!(we, tauri::WindowEvent::Destroyed) {
                        diag(&format!("WindowEvent::Destroyed on '{label}'"));
                    }
                }
                tauri::RunEvent::ExitRequested { code, .. } => {
                    diag(&format!("ExitRequested code={code:?}"));
                }
                tauri::RunEvent::Exit => diag("Exit"),
                _ => {}
            }
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                if let Some(db) = app.try_state::<Db>() {
                    db.finalize_on_exit();
                }
            }
        });
}
