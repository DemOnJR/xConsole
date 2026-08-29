//! Settings surface for remote control.
//!
//! Bot tokens are credentials that grant control of the agent, so they follow the same
//! rule as every other secret here: they go to the OS keychain and are never returned
//! to the frontend. The UI can see *whether* one is set, never what it is. WhatsApp has
//! no token at all — it authenticates by a paired device, which is why its section of
//! the UI is a QR code rather than a password field.

use tauri::{AppHandle, State};

use crate::ai::remote::{self, whatsapp, Kind};
use crate::storage::Db;

/// One transport's configuration, as the settings screen sees it.
#[derive(serde::Serialize)]
pub struct TransportStatus {
    /// "discord" | "telegram" | "whatsapp".
    pub kind: String,
    pub enabled: bool,
    pub chat_id: String,
    pub allowed_user_ids: String,
    pub has_token: bool,
    /// Whether this platform needs a credential pasted in at all.
    pub needs_token: bool,
    /// Whether this platform refuses to arm without a chat id.
    pub chat_required: bool,
    /// False when this transport would refuse to run — the UI explains why rather than
    /// leaving the user staring at a bridge that silently does nothing.
    pub usable: bool,
}

#[derive(serde::Serialize)]
pub struct RemoteStatus {
    /// The master switch. Every transport is off while this is.
    pub enabled: bool,
    pub prefix: String,
    pub safety_mode: String,
    pub targets: Vec<String>,
    pub transports: Vec<TransportStatus>,
    /// True when at least one transport is armed.
    pub usable: bool,
    /// Which transport the shared conversation is currently on — where the user last
    /// spoke, and where an unprompted message would go. `None` until someone writes.
    pub last_route: Option<String>,
    /// How many messages the shared thread is carrying. Surfaced so "start a new
    /// conversation" is an informed choice rather than a button with unknown effect.
    pub conversation_len: usize,
}

fn transport_status(db: &Db, kind: Kind) -> TransportStatus {
    let cfg = remote::load_config(db, kind);
    let needs_token = kind.secret_key().is_some();
    let has_token = remote::load_token(kind).is_some();
    TransportStatus {
        kind: kind.as_str().to_string(),
        enabled: db
            .get_setting(&kind.setting_enabled())
            .ok()
            .flatten()
            .map(|v| v == "true")
            // Discord predates the per-transport switch; see `remote::load_config`.
            .unwrap_or(kind == Kind::Discord),
        chat_id: cfg.chat_id.clone(),
        allowed_user_ids: cfg.allowed_user_ids.join(", "),
        // A transport that needs no credential is never "missing" one; reporting false
        // here would make the WhatsApp row permanently look broken.
        usable: cfg.is_usable() && (!needs_token || has_token),
        has_token,
        needs_token,
        chat_required: kind.chat_required(),
    }
}

fn status(db: &Db) -> RemoteStatus {
    let get = |k: &str| db.get_setting(k).ok().flatten().unwrap_or_default();
    let transports: Vec<TransportStatus> = Kind::ALL.iter().map(|k| transport_status(db, *k)).collect();
    RemoteStatus {
        enabled: get(remote::SETTING_ENABLED) == "true",
        prefix: get(remote::SETTING_PREFIX),
        safety_mode: remote::load_config(db, Kind::Discord).safety_mode,
        targets: remote::parse_id_list(&get(remote::SETTING_TARGETS)),
        usable: transports.iter().any(|t| t.usable),
        transports,
        last_route: remote::last_route(db).map(|r| r.kind.as_str().to_string()),
        conversation_len: remote::load_history(db).len(),
    }
}

#[tauri::command]
pub async fn get_remote_status(db: State<'_, Db>) -> Result<RemoteStatus, String> {
    Ok(status(&db))
}

/// The settings shared by every transport.
///
/// The settings screen sends these fields camelCased. Tauri only maps the casing of a
/// command's own argument names, never the fields inside them, so this rename is what
/// makes `safetyMode` land in `safety_mode`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteShared {
    pub enabled: bool,
    pub prefix: String,
    pub safety_mode: String,
    pub targets: Vec<String>,
}

/// One transport's settings. `token` of `None` or `""` means "leave the stored
/// credential alone", so the UI can save other fields without re-entering something it
/// is never shown.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportInput {
    pub kind: String,
    pub enabled: bool,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub allowed_user_ids: String,
    #[serde(default)]
    pub token: Option<String>,
}

/// Validate and store one transport's configuration.
///
/// Split out from the command so the agent's own `remote_configure` tool goes through
/// exactly the same refusals. A second, laxer path into this setting is how an
/// allowlist ends up empty on a bridge that is armed.
pub fn apply_transport(db: &Db, input: &TransportInput) -> Result<(), String> {
    let kind = Kind::parse(&input.kind).ok_or_else(|| format!("unknown transport {}", input.kind))?;
    let allowed = remote::parse_id_list(&input.allowed_user_ids);

    // Refuse to arm a bridge that would accept commands from nobody in particular.
    // Saying so here beats letting the user believe remote control is on.
    if input.enabled && allowed.is_empty() {
        return Err(format!(
            "list at least one {} identity that may command the agent — an empty list \
             would mean nobody, and this must never mean everyone",
            kind.as_str()
        ));
    }
    if input.enabled && kind.chat_required() && input.chat_id.trim().is_empty() {
        return Err(format!("{} needs a channel id", kind.as_str()));
    }

    let set = |k: &str, v: &str| db.set_setting(k, v).map_err(|e| e.to_string());
    set(&kind.setting_enabled(), if input.enabled { "true" } else { "false" })?;
    set(&kind.setting_chat(), input.chat_id.trim())?;
    set(&kind.setting_allowed(), &allowed.join(","))?;

    if let Some(token) = input.token.as_ref().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let key = kind
            .secret_key()
            .ok_or_else(|| format!("{} does not use a token", kind.as_str()))?;
        crate::secrets::set_secret(&key, token).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Store the shared settings.
pub fn apply_shared(db: &Db, shared: &RemoteShared) -> Result<(), String> {
    let set = |k: &str, v: &str| db.set_setting(k, v).map_err(|e| e.to_string());
    set(remote::SETTING_ENABLED, if shared.enabled { "true" } else { "false" })?;
    set(remote::SETTING_PREFIX, shared.prefix.trim())?;
    set(remote::SETTING_SAFETY, shared.safety_mode.trim())?;
    set(remote::SETTING_TARGETS, &shared.targets.join(","))
}

#[tauri::command]
pub async fn save_remote_config(
    db: State<'_, Db>,
    shared: RemoteShared,
    transports: Vec<TransportInput>,
) -> Result<RemoteStatus, String> {
    for t in &transports {
        apply_transport(&db, t)?;
    }
    apply_shared(&db, &shared)?;
    Ok(status(&db))
}

/// Forget the shared remote thread.
///
/// The bridge keeps one conversation across all three transports, so it accumulates until
/// something clears it. This is both the "start fresh" button and the way to get rid of a
/// thread you would rather not have sitting in the database.
#[tauri::command]
pub async fn reset_remote_conversation(db: State<'_, Db>) -> Result<RemoteStatus, String> {
    db.delete_agent_conversation(remote::CONVERSATION_ID)
        .map_err(|e| e.to_string())?;
    Ok(status(&db))
}

#[tauri::command]
pub async fn clear_remote_token(db: State<'_, Db>, kind: String) -> Result<RemoteStatus, String> {
    let kind = Kind::parse(&kind).ok_or_else(|| format!("unknown transport {kind}"))?;
    if let Some(key) = kind.secret_key() {
        crate::secrets::delete_secret(&key).map_err(|e| e.to_string())?;
    }
    // A transport with no credential cannot run, and leaving it switched on would
    // report "armed" for a bridge that will never connect.
    db.set_setting(&kind.setting_enabled(), "false").map_err(|e| e.to_string())?;
    Ok(status(&db))
}

/// Check a saved bot token by asking the platform who it belongs to.
///
/// Turns "why is nothing happening?" into a stated answer. Only Telegram supports it
/// so far; the others report that they cannot be checked rather than pretending to
/// have passed.
#[tauri::command]
pub async fn test_remote_token(kind: String) -> Result<String, String> {
    let kind = Kind::parse(&kind).ok_or_else(|| format!("unknown transport {kind}"))?;
    let token = remote::load_token(kind).ok_or("no token saved")?;
    match kind {
        Kind::Telegram => {
            let who = crate::ai::remote::telegram::get_me(&token).await?;
            Ok(format!("Connected as {who}"))
        }
        _ => Err(format!("{} tokens cannot be checked from here", kind.as_str())),
    }
}

// ---------------------------------------------------------------------------
// WhatsApp pairing
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn whatsapp_status(app: AppHandle) -> Result<whatsapp::WhatsAppStatus, String> {
    Ok(whatsapp::status(&app).await)
}

/// Start the sidecar and surface a QR code. Progress arrives on the
/// `remote://whatsapp` event rather than as a return value, because pairing takes as
/// long as the user takes to find their phone.
#[tauri::command]
pub async fn whatsapp_link_start(app: AppHandle) -> Result<whatsapp::WhatsAppStatus, String> {
    whatsapp::link_start(&app).await
}

#[tauri::command]
pub async fn whatsapp_link_cancel(app: AppHandle) -> Result<whatsapp::WhatsAppStatus, String> {
    Ok(whatsapp::link_cancel(&app).await)
}

#[tauri::command]
pub async fn whatsapp_unlink(app: AppHandle) -> Result<whatsapp::WhatsAppStatus, String> {
    whatsapp::unlink(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload `api.saveRemoteConfig` sends, field for field.
    ///
    /// These two structs are the only place in the module where the webview's names have
    /// to line up with Rust's, and nothing else checks it: Tauri renames a command's own
    /// arguments but not the fields inside them, so a missing `rename_all` fails at the
    /// IPC boundary with "missing field `safety_mode`" — or, for the fields carrying
    /// `#[serde(default)]`, does not fail at all and quietly saves an empty allowlist.
    #[test]
    fn the_settings_screen_payload_deserializes() {
        let shared: RemoteShared = serde_json::from_str(
            r#"{"enabled":true,"prefix":"!x","safetyMode":"allowlist","targets":["vps-1"]}"#,
        )
        .expect("shared settings");
        assert_eq!(shared.safety_mode, "allowlist");
        assert_eq!(shared.targets, vec!["vps-1"]);

        let t: TransportInput = serde_json::from_str(
            r#"{"kind":"telegram","enabled":true,"chatId":"-100123",
                "allowedUserIds":"@ada, +40712345678","token":null}"#,
        )
        .expect("transport settings");
        assert_eq!(t.chat_id, "-100123");
        assert_eq!(t.allowed_user_ids, "@ada, +40712345678");
        assert!(t.token.is_none());
    }
}
