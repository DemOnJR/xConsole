use tauri::State;

use crate::ssh::remote_ops::{
    self, RemoteFileStat,
};
use crate::ssh::SessionManager;

#[tauri::command]
pub async fn vps_file_stat(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
) -> Result<RemoteFileStat, String> {
    remote_ops::stat_file(&sessions, &vps_id, &path).await
}

#[tauri::command]
pub async fn vps_file_chmod(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
    mode: String,
    recursive: bool,
) -> Result<(), String> {
    remote_ops::chmod(&sessions, &vps_id, &path, &mode, recursive)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn vps_file_chown(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
    owner: String,
    group: String,
    recursive: bool,
) -> Result<(), String> {
    remote_ops::chown(&sessions, &vps_id, &path, &owner, &group, recursive)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn vps_file_delete(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    remote_ops::delete_path(&sessions, &vps_id, &path, is_dir)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn vps_file_rename(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    remote_ops::rename_path(&sessions, &vps_id, &from, &to)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn vps_file_mkdir(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
) -> Result<(), String> {
    remote_ops::mkdir_path(&sessions, &vps_id, &path).await.map(|_| ())
}

#[tauri::command]
pub async fn vps_file_touch(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
) -> Result<(), String> {
    remote_ops::touch_file(&sessions, &vps_id, &path).await.map(|_| ())
}
