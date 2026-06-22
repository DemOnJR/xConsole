use tauri::State;

use crate::secrets;
use crate::storage::models::{AiProvider, AiProviderInput};
use crate::storage::Db;

/// A single key/value setting, used for list responses.
#[derive(serde::Serialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

#[tauri::command]
pub fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_settings(db: State<'_, Db>) -> Result<Vec<Setting>, String> {
    let rows = db.list_settings().map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(key, value)| Setting { key, value })
        .collect())
}

#[tauri::command]
pub fn delete_setting(db: State<'_, Db>, key: String) -> Result<(), String> {
    db.delete_setting(&key).map_err(|e| e.to_string())
}

// ----- AI providers -----

#[tauri::command]
pub fn list_providers(db: State<'_, Db>) -> Result<Vec<AiProvider>, String> {
    db.list_providers().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_provider(db: State<'_, Db>, input: AiProviderInput) -> Result<AiProvider, String> {
    let secret = input.secret.clone();
    let provider = db.upsert_provider(&input).map_err(|e| e.to_string())?;

    // The API key / token goes only to the OS keychain. An empty string clears it.
    if let Some(secret) = secret {
        let key = secrets::provider_key(&provider.id);
        if secret.is_empty() {
            let _ = secrets::delete_secret(&key);
        } else {
            secrets::set_secret(&key, &secret).map_err(|e| e.to_string())?;
        }
    }

    db.get_provider(&provider.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "provider vanished after save".to_string())
}

#[tauri::command]
pub fn delete_provider(db: State<'_, Db>, id: String) -> Result<(), String> {
    let _ = secrets::delete_secret(&secrets::provider_key(&id));
    db.delete_provider(&id).map_err(|e| e.to_string())
}
