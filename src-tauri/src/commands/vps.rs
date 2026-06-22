use tauri::State;

use crate::secrets;
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

#[tauri::command]
pub fn delete_vps(db: State<'_, Db>, id: String) -> Result<(), String> {
    let _ = secrets::delete_secret(&id);
    db.delete_vps(&id).map_err(|e| e.to_string())
}
