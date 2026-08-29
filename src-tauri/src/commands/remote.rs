//! Settings surface for remote control (Discord).
//!
//! The bot token is a credential that grants control of the agent, so it follows the
//! same rule as every other secret here: it goes to the OS keychain and is never
//! returned to the frontend. The UI can see *whether* one is set, never what it is.

use tauri::State;

use crate::ai::remote;
use crate::storage::Db;

#[derive(serde::Serialize)]
pub struct RemoteStatus {
    pub enabled: bool,
    pub channel_id: String,
    pub allowed_user_ids: String,
    pub prefix: String,
    pub safety_mode: String,
    pub targets: Vec<String>,
    pub has_token: bool,
    /// False when the configuration would refuse to run — the UI explains why
    /// rather than leaving the user staring at a bridge that silently does nothing.
    pub usable: bool,
}

#[tauri::command]
pub async fn get_remote_status(db: State<'_, Db>) -> Result<RemoteStatus, String> {
    let cfg = remote::load_config(&db);
    let has_token = crate::secrets::get_secret(remote::SECRET_DISCORD_TOKEN)
        .ok()
        .flatten()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    Ok(RemoteStatus {
        enabled: cfg.enabled,
        channel_id: cfg.channel_id.clone(),
        allowed_user_ids: cfg.allowed_user_ids.join(", "),
        prefix: cfg.prefix.clone(),
        safety_mode: cfg.safety_mode.clone(),
        targets: cfg.targets.clone(),
        usable: cfg.is_usable() && has_token,
        has_token,
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn save_remote_config(
    db: State<'_, Db>,
    enabled: bool,
    channel_id: String,
    allowed_user_ids: String,
    prefix: String,
    safety_mode: String,
    targets: Vec<String>,
    token: Option<String>,
) -> Result<RemoteStatus, String> {
    let allowed = remote::parse_id_list(&allowed_user_ids);
    // Refuse to arm a bridge that would accept commands from nobody in particular.
    // Saying so here beats letting the user believe remote control is on.
    if enabled && allowed.is_empty() {
        return Err(
            "list at least one Discord user id that may command the agent — an empty list \
             would mean nobody, and this must never mean everyone"
                .into(),
        );
    }
    if enabled && channel_id.trim().is_empty() {
        return Err("a channel id is required".into());
    }

    let set = |k: &str, v: &str| db.set_setting(k, v).map_err(|e| e.to_string());
    set(remote::SETTING_ENABLED, if enabled { "true" } else { "false" })?;
    set(remote::SETTING_CHANNEL, channel_id.trim())?;
    set(remote::SETTING_ALLOWED, &allowed.join(","))?;
    set(remote::SETTING_PREFIX, prefix.trim())?;
    set(remote::SETTING_SAFETY, safety_mode.trim())?;
    set(remote::SETTING_TARGETS, &targets.join(","))?;

    // An empty string means "leave the stored token alone", so the UI can save other
    // fields without having to re-enter a credential it is never shown.
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        crate::secrets::set_secret(remote::SECRET_DISCORD_TOKEN, token.trim())
            .map_err(|e| e.to_string())?;
    }
    get_remote_status(db).await
}

#[tauri::command]
pub async fn clear_remote_token() -> Result<(), String> {
    crate::secrets::delete_secret(remote::SECRET_DISCORD_TOKEN).map_err(|e| e.to_string())
}
