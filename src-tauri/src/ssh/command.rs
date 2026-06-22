//! Headless VPS command execution (agent MCP subprocess, no Tauri UI).

use std::time::Duration;

use russh::{ChannelMsg, Disconnect};

use crate::secrets;
use crate::storage::models::{AuthType, Vps};
use crate::storage::Db;

use super::client::{self, Auth, ConnectError};
use super::manager::CommandOutput;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

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

    let auth = resolve_auth(&vps).map_err(|e| e.to_string())?;
    let connected = client::connect(&vps.host, vps.port, &vps.username, auth, db.clone())
        .await
        .map_err(|e| e.to_string())?;
    let handle = connected.handle;

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

fn resolve_auth(vps: &Vps) -> Result<Auth, ConnectError> {
    match vps.auth_type {
        AuthType::Agent => Ok(Auth::Agent),
        AuthType::Password => {
            let secret = secrets::get_secret(&vps.id)
                .map_err(|e| ConnectError::Other(e.to_string()))?
                .ok_or_else(|| ConnectError::MissingCredential("password".into()))?;
            Ok(Auth::Password(secret.to_string()))
        }
        AuthType::Key => {
            let key_path = vps
                .key_path
                .clone()
                .ok_or_else(|| ConnectError::MissingCredential("key_path".into()))?;
            let passphrase = secrets::get_secret(&vps.id)
                .map_err(|e| ConnectError::Other(e.to_string()))?
                .map(|z| z.to_string());
            Ok(Auth::Key {
                key_path,
                passphrase,
            })
        }
    }
}
