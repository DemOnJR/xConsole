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

#[derive(serde::Serialize)]
pub struct AutostartStatus {
    pub enabled: bool,
    pub supported: bool,
}

/// Whether xConsole launches when this user signs in to Windows.
#[tauri::command]
pub fn get_autostart() -> Result<AutostartStatus, String> {
    Ok(AutostartStatus {
        enabled: crate::autostart::is_enabled()?,
        supported: crate::autostart::is_supported(),
    })
}

/// Turn launch-at-sign-in on or off. Writes the current executable into the
/// per-user Run key; no admin, and uninstall removes it.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<AutostartStatus, String> {
    crate::autostart::set_enabled(enabled)?;
    get_autostart()
}

/// Write a line into `xconsole.log` from the frontend.
///
/// Exists to answer one question that the Rust-side events cannot: when the window gets a
/// `CloseRequested`, was it the app's own title-bar button, or did the OS send WM_CLOSE
/// from outside? Both look identical from inside the event loop.
#[tauri::command]
pub fn log_diag(message: String) {
    // Cap it: this is reachable from the webview, and an unbounded string would let a
    // runaway caller fill the log.
    let msg: String = message.chars().take(300).collect();
    crate::diag(&format!("ui: {msg}"));
}
