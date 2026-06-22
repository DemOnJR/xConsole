use tauri::State;

use crate::ssh::{SftpConnectOutcome, SftpListOutcome, SftpManager};
use crate::storage::Db;

#[tauri::command]
pub async fn sftp_connect(
    sftp: State<'_, SftpManager>,
    db: State<'_, Db>,
    vps_id: String,
) -> Result<SftpConnectOutcome, String> {
    let _ = db;
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
pub fn sftp_disconnect(sftp: State<'_, SftpManager>, session_id: String) -> Result<(), String> {
    sftp.disconnect(&session_id)
}
