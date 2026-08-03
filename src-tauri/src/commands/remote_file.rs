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

/// Delete a selection. One command for the whole selection.
#[tauri::command]
pub async fn vps_file_delete_many(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    paths: Vec<String>,
) -> Result<(), String> {
    remote_ops::delete_many(&sessions, &vps_id, &paths).await.map(|_| ())
}

/// Copy or move a selection into a directory. One command for the whole selection.
#[tauri::command]
pub async fn vps_file_copy(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    sources: Vec<String>,
    dest_dir: String,
    move_them: bool,
) -> Result<(), String> {
    remote_ops::copy_into(&sessions, &vps_id, &sources, &dest_dir, move_them)
        .await
        .map(|_| ())
}

/// Search under `root` by name and/or extension, optionally through subdirectories.
#[tauri::command]
pub async fn vps_file_search(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    root: String,
    pattern: String,
    extensions: Vec<String>,
    recursive: bool,
) -> Result<Vec<String>, String> {
    remote_ops::search(&sessions, &vps_id, &root, &pattern, &extensions, recursive).await
}

/// Create a symlink at `path` pointing at `target`, or repoint an existing one.
///
/// One command for both because `ln -sfn` is one command for both, and because "change
/// where this link points" is the operation people actually want — the alternative is
/// delete-then-create, which loses the link if the second step fails.
#[tauri::command]
pub async fn vps_file_symlink(
    sessions: State<'_, SessionManager>,
    vps_id: String,
    path: String,
    target: String,
) -> Result<(), String> {
    remote_ops::symlink(&sessions, &vps_id, &path, &target)
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
