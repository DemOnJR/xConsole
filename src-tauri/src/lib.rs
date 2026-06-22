mod ai;
mod commands;
mod infra;
pub mod mcp;
mod secrets;
mod ssh;
mod storage;

use ai::safety::ApprovalRegistry;
use ai::AgentHome;
use ssh::{SessionManager, SftpManager};
use storage::Db;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Local database under the app data dir.
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"));
            let db_path = dir.join("xconsole.db");
            let db = Db::open(&db_path).expect("failed to open database");

            let handle = app.handle().clone();
            let sessions = SessionManager::new(handle, db.clone());
            let sftp = SftpManager::new(db.clone());

            // Agent home: editable Hermes-format files (SOUL.md / MEMORY.md / ...).
            let agent_home = AgentHome::new(dir.join("agent"));
            ai::skills::seed_defaults(&agent_home);

            let approvals = ApprovalRegistry::new();
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

            app.manage(db);
            app.manage(sessions);
            app.manage(sftp);
            app.manage(agent_home);
            app.manage(approvals);
            app.manage(cron_running);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vps::list_vps,
            commands::vps::save_vps,
            commands::vps::delete_vps,
            commands::session::ssh_connect,
            commands::session::ssh_write,
            commands::session::ssh_resize,
            commands::session::ssh_disconnect,
            commands::session::ssh_replay,
            commands::sftp::sftp_connect,
            commands::sftp::sftp_list,
            commands::sftp::sftp_download,
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
            commands::workspace::list_known_hosts,
            commands::workspace::forget_host_key,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::list_settings,
            commands::settings::delete_setting,
            commands::settings::list_providers,
            commands::settings::save_provider,
            commands::settings::delete_provider,
            commands::ai::ai_cli_login,
            commands::ai::ai_chat,
            commands::ai::list_agent_conversations,
            commands::ai::get_agent_conversation,
            commands::ai::save_agent_conversation,
            commands::ai::delete_agent_conversation,
            commands::ai::agent_resolve_approval,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
