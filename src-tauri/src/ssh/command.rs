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
    run_vps_command_for(db, vps_id, command, COMMAND_TIMEOUT).await
}

/// Longest a caller may wait for one command.
///
/// Not a safety limit — the work is happening either way — so it exists only to stop a
/// hung command holding a turn open forever. An hour is past the point where waiting
/// synchronously was the right choice at all; use `background: true` beyond it.
pub const MAX_COMMAND_TIMEOUT: Duration = Duration::from_secs(3600);

/// Run one command with a caller-chosen deadline.
///
/// The fixed two minutes was the right default and the wrong ceiling: a build, an
/// `npm install`, a migration or a large rsync passes it routinely, and the only reply
/// was "timed out" for work that was in fact progressing. The caller now says how long
/// it is prepared to wait, and the error says what to do when that runs out.
pub async fn run_vps_command_for(
    db: &Db,
    vps_id: &str,
    command: &str,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let timeout = timeout.min(MAX_COMMAND_TIMEOUT);
    match tokio::time::timeout(timeout, run_vps_command_inner(db, vps_id, command)).await {
        Ok(r) => r,
        Err(_) => Err(format!(
            "command timed out after {}s. It may still be running on the server. For work \
             that takes this long, start it with background:true instead — the job survives \
             the turn, the session and an xConsole restart, and job_status reports on it.",
            timeout.as_secs()
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
