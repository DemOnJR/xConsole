use base64::Engine;
use tauri::State;

use crate::ssh::{ConnectOutcome, SessionManager};
use crate::storage::Db;

#[tauri::command]
pub async fn ssh_connect(
    sessions: State<'_, SessionManager>,
    db: State<'_, Db>,
    vps_id: String,
    cols: u32,
    rows: u32,
) -> Result<ConnectOutcome, String> {
    let vps = db
        .get_vps(&vps_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VPS not found".to_string())?;
    sessions.connect(vps, cols.max(1), rows.max(1)).await
}

#[tauri::command]
pub fn ssh_write(
    sessions: State<'_, SessionManager>,
    session_id: String,
    data_b64: String,
) -> Result<(), String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| e.to_string())?;
    sessions.write(&session_id, &data)
}

#[tauri::command]
pub fn ssh_resize(
    sessions: State<'_, SessionManager>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    sessions.resize(&session_id, cols.max(1), rows.max(1))
}

#[tauri::command]
pub fn ssh_disconnect(
    sessions: State<'_, SessionManager>,
    session_id: String,
) -> Result<(), String> {
    sessions.disconnect(&session_id)
}

/// Base64 of recent output for replay (re-focus / reconnect).
#[tauri::command]
pub fn ssh_replay(sessions: State<'_, SessionManager>, session_id: String) -> Option<String> {
    sessions.replay(&session_id)
}
