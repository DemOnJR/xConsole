use tauri::State;

use crate::secrets;
use crate::ssh::keygen::{self, SetupReport};
use crate::ssh::SessionManager;
use crate::storage::models::{Vps, VpsInput};
use crate::storage::Db;

#[tauri::command]
pub fn list_vps(db: State<'_, Db>) -> Result<Vec<Vps>, String> {
    db.list_vps().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_vps(db: State<'_, Db>, input: VpsInput) -> Result<Vps, String> {
    // Persist the non-secret fields to SQLite.
    let vps = db.upsert_vps(&input).map_err(|e| e.to_string())?;

    // Store the secret (password / key passphrase) only in the OS keychain.
    if let Some(secret) = input.secret {
        if secret.is_empty() {
            let _ = secrets::delete_secret(&vps.id);
        } else {
            secrets::set_secret(&vps.id, &secret).map_err(|e| e.to_string())?;
        }
    }

    Ok(vps)
}

/// Persist a manual ordering of the server sidebar.
#[tauri::command]
pub fn reorder_vps(db: State<'_, Db>, ids: Vec<String>) -> Result<(), String> {
    db.reorder_vps(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_vps(db: State<'_, Db>, id: String) -> Result<(), String> {
    let _ = secrets::delete_secret(&id);
    // Also purge any app-managed SSH private key for this VPS.
    let _ = secrets::delete_secret(&secrets::ssh_key_key(&id));
    db.delete_vps(&id).map_err(|e| e.to_string())
}

/// Switch a VPS from password to an app-managed SSH key (generate, install,
/// verify). Shared with the agent's `ssh_setup_key_auth` tool via
/// [`keygen::setup_key_auth`]. The private key is stored only in the OS keychain.
#[tauri::command]
pub async fn setup_vps_key_auth(
    db: State<'_, Db>,
    sessions: State<'_, SessionManager>,
    vps_id: String,
) -> Result<SetupReport, String> {
    // Clone state out of the guards so nothing non-Send is held across await.
    let db = (*db).clone();
    let sessions = (*sessions).clone();
    keygen::setup_key_auth(&db, &sessions, &vps_id).await
}
