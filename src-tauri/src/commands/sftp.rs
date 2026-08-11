use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::ssh::external_edit::ExternalEditHandle;
use crate::ssh::transfer::{
    ArchiveFormat, Direction, TransferManager, TransferSnapshot, DEFAULT_CONCURRENCY,
};
use crate::ssh::{SftpConnectOutcome, SftpListOutcome, SftpManager};

#[tauri::command]
pub async fn sftp_connect(
    sftp: State<'_, SftpManager>,
    vps_id: String,
) -> Result<SftpConnectOutcome, String> {
    sftp.connect(&vps_id).await
}

#[tauri::command]
pub async fn sftp_list(
    sftp: State<'_, SftpManager>,
    session_id: String,
    path: String,
) -> Result<SftpListOutcome, String> {
    sftp.list(&session_id, &path).await
}

#[tauri::command]
pub async fn sftp_download(
    sftp: State<'_, SftpManager>,
    session_id: String,
    path: String,
) -> Result<String, String> {
    sftp.download(&session_id, &path).await
}

#[tauri::command]
pub async fn sftp_write(
    sftp: State<'_, SftpManager>,
    session_id: String,
    path: String,
    content_b64: String,
) -> Result<(), String> {
    sftp.write(&session_id, &path, &content_b64).await
}

#[tauri::command]
pub async fn sftp_mkdir(
    sftp: State<'_, SftpManager>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    sftp.mkdir(&session_id, &path).await
}

#[tauri::command]
pub async fn sftp_rename(
    sftp: State<'_, SftpManager>,
    session_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    sftp.rename(&session_id, &from, &to).await
}

#[tauri::command]
pub async fn sftp_remove(
    sftp: State<'_, SftpManager>,
    session_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    sftp.remove(&session_id, &path, is_dir).await
}

#[tauri::command]
pub async fn sftp_symlink(
    sftp: State<'_, SftpManager>,
    session_id: String,
    link_path: String,
    target: String,
) -> Result<(), String> {
    sftp.symlink(&session_id, &link_path, &target).await
}

#[tauri::command]
pub fn sftp_disconnect(sftp: State<'_, SftpManager>, session_id: String) -> Result<(), String> {
    sftp.disconnect(&session_id)
}

// ----- Bulk transfers -----

/// Ask the user where downloads should go. `None` when they cancel.
///
/// The picker runs in Rust rather than the webview so the frontend needs no filesystem
/// or dialog capability — the only local paths it ever learns are ones the user chose.
#[tauri::command]
pub async fn pick_directory(app: AppHandle, title: String) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_folder(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });
    rx.await.map_err(|_| "folder picker closed unexpectedly".to_string())
}

/// Ask the user which local files to upload. Empty when they cancel.
#[tauri::command]
pub async fn pick_files(app: AppHandle, title: String) -> Result<Vec<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_files(move |paths| {
            let _ = tx.send(
                paths
                    .map(|list| list.into_iter().map(|p| p.to_string()).collect())
                    .unwrap_or_default(),
            );
        });
    rx.await.map_err(|_| "file picker closed unexpectedly".to_string())
}

/// Begin a download or upload. Returns the job id; progress arrives on `sftp://transfer`.
#[tauri::command]
pub fn sftp_transfer_start(
    app: AppHandle,
    transfers: State<'_, TransferManager>,
    sftp: State<'_, SftpManager>,
    session_id: String,
    direction: Direction,
    sources: Vec<String>,
    destination: String,
    concurrency: Option<usize>,
) -> Result<String, String> {
    crate::ssh::transfer::spawn_job(
        app,
        transfers.inner().clone(),
        sftp.inner().clone(),
        session_id,
        direction,
        sources,
        destination,
        concurrency.unwrap_or(DEFAULT_CONCURRENCY),
    )
}

/// Download a remote directory as one archive (built on the server, then transferred).
#[tauri::command]
pub fn sftp_archive_start(
    app: AppHandle,
    transfers: State<'_, TransferManager>,
    sftp: State<'_, SftpManager>,
    session_id: String,
    remote_dir: String,
    destination: String,
    format: ArchiveFormat,
) -> Result<String, String> {
    crate::ssh::transfer::spawn_archive_job(
        app,
        transfers.inner().clone(),
        sftp.inner().clone(),
        session_id,
        remote_dir,
        destination,
        format,
        1,
    )
}

#[tauri::command]
pub fn sftp_transfer_cancel(transfers: State<'_, TransferManager>, id: String) -> Result<(), String> {
    transfers.cancel(&id)
}

/// Every job the app still knows about — used when the transfers panel mounts.
#[tauri::command]
pub fn sftp_transfer_list(transfers: State<'_, TransferManager>) -> Vec<TransferSnapshot> {
    transfers.list()
}

#[tauri::command]
pub fn sftp_transfer_clear_finished(transfers: State<'_, TransferManager>) {
    transfers.clear_finished();
}

/// Open a remote file in the configured external editor and push every save back.
#[tauri::command]
pub async fn sftp_edit_external(
    app: AppHandle,
    sftp: State<'_, SftpManager>,
    db: State<'_, crate::storage::Db>,
    session_id: String,
    path: String,
) -> Result<ExternalEditHandle, String> {
    let setting = db
        .get_setting(EXTERNAL_EDITOR_SETTING)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if setting.trim().is_empty() {
        return Err(
            "No external editor is set. Add one in Settings → General (for example: code)."
                .into(),
        );
    }
    crate::ssh::external_edit::start(app, sftp.inner().clone(), session_id, path, setting).await
}

/// Settings key holding the external editor command (e.g. `code`).
pub const EXTERNAL_EDITOR_SETTING: &str = "sftp.external_editor";
