//! Headless VPS command execution (agent MCP subprocess, no Tauri UI).

use std::time::Duration;

use russh::client::Handle;
use russh::{ChannelMsg, Disconnect};

use crate::storage::Db;

use super::client::{self, Handler};
use super::manager::CommandOutput;

/// Maximum wall-clock time for a non-interactive SSH command (agent/cron/MCP).
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Run a shell command on a VPS using stored credentials (keychain / key path).
pub async fn run_vps_command(
    db: &Db,
    vps_id: &str,
    command: &str,
) -> Result<CommandOutput, String> {
    match tokio::time::timeout(COMMAND_TIMEOUT, run_vps_command_inner(db, vps_id, command)).await {
        Ok(r) => r,
        Err(_) => Err(format!(
            "command timed out after {}s",
            COMMAND_TIMEOUT.as_secs()
        )),
    }
}

async fn run_vps_command_inner(
    db: &Db,
    vps_id: &str,
    command: &str,
) -> Result<CommandOutput, String> {
    let vps = db
        .get_vps(vps_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VPS not found".to_string())?;

    let auth = client::resolve_auth(&vps).map_err(|e| e.to_string())?;
    let connected = client::connect(&vps.host, vps.port, &vps.username, auth, db.clone())
        .await
        .map_err(|e| e.to_string())?;
    run_on_handle(connected.handle, command).await
}

/// Open a fresh channel on an authenticated handle, run one command to
/// completion, and capture stdout/stderr/exit code. Shared by the headless
/// command path and [`super::manager::SessionManager::run_command`].
pub(super) async fn run_on_handle(
    handle: Handle<Handler>,
    command: &str,
) -> Result<CommandOutput, String> {
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| e.to_string())?;
    channel.exec(true, command).await.map_err(|e| e.to_string())?;

    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut exit_code: Option<i32> = None;

    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { ref data }) => stdout.extend_from_slice(data),
            Some(ChannelMsg::ExtendedData { ref data, ext }) => {
                if ext == 1 {
                    stderr.extend_from_slice(data);
                } else {
                    stdout.extend_from_slice(data);
                }
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                exit_code = Some(exit_status as i32);
            }
            Some(ChannelMsg::Eof) => {
                if exit_code.is_some() {
                    break;
                }
            }
            Some(ChannelMsg::Close) | None => break,
            _ => {}
        }
    }

    let _ = handle
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: exit_code.unwrap_or(-1),
    })
}
