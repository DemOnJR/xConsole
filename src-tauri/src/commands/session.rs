use base64::Engine;
use tauri::State;

use crate::ssh::{shell_quote, ConnectOutcome, SessionManager};
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

/// Git branch for a remote directory, if it is (or is inside) a git work tree.
///
/// Returns `None` when the path is not a repo, `git` is missing, or the command fails.
/// Detached HEAD is reported as a short SHA prefixed with `detached@`.
#[tauri::command]
pub async fn remote_git_branch(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
) -> Result<Option<String>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    // One short shell script: resolve work-tree root, then branch (or short SHA).
    // Silent on non-repos so the UI can call this on every cwd/path change.
    let q = shell_quote(path);
    let cmd = format!(
        "d={q}; \
         git -C \"$d\" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0; \
         b=$(git -C \"$d\" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0; \
         if [ \"$b\" = \"HEAD\" ]; then \
           s=$(git -C \"$d\" rev-parse --short HEAD 2>/dev/null) || exit 0; \
           echo \"detached@$s\"; \
         else \
           echo \"$b\"; \
         fi"
    );
    let out = sessions.run_command(&vps_id, &cmd).await?;
    let branch = out
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|s| s.to_string());
    Ok(branch)
}
