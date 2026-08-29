//! Remote control: drive the agent from a chat app while xConsole is running.
//!
//! The point is to reach your servers from your phone without xConsole being
//! reachable *from* the internet. So this polls outbound over HTTPS and never listens
//! — no port, no tunnel, no inbound firewall rule. Close xConsole and remote control
//! stops existing, which is the intended failure mode.
//!
//! # Threat model
//!
//! This turns a chat message into commands on the user's infrastructure, so the
//! authorisation decision is the whole feature. Every rule here fails closed:
//!
//! - Off unless explicitly enabled.
//! - An empty allowlist authorises **nobody**. "No list configured" must never mean
//!   "anyone in the channel", which is the mistake that would hand a server to
//!   whoever wanders into a Discord.
//! - Only the one configured channel is read, so being added to another channel
//!   grants nothing.
//! - Bot messages are ignored, including our own replies — otherwise a reply that
//!   happens to contain the prefix drives the next turn, and the agent talks to
//!   itself until the tokens run out.
//! - The safety mode still applies. Nobody can answer an approval prompt from a
//!   phone, so a remote turn runs under its own mode and defaults to the strictest.

use serde::{Deserialize, Serialize};

/// Longest reply a single Discord message can carry.
const DISCORD_MAX_CHARS: usize = 2000;

/// Settings keys. Kept together so the UI, the poller and the tests cannot drift.
pub const SETTING_ENABLED: &str = "remote.enabled";
pub const SETTING_CHANNEL: &str = "remote.discord.channel_id";
pub const SETTING_ALLOWED: &str = "remote.discord.allowed_user_ids";
pub const SETTING_PREFIX: &str = "remote.prefix";
pub const SETTING_SAFETY: &str = "remote.safety_mode";
pub const SETTING_TARGETS: &str = "remote.targets";
/// Keychain entry holding the bot token.
pub const SECRET_DISCORD_TOKEN: &str = "remote.discord.token";

/// How the remote bridge is configured. Built from settings; see [`Config::is_usable`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub channel_id: String,
    /// Chat-platform user ids permitted to command the agent.
    pub allowed_user_ids: Vec<String>,
    /// Messages must start with this to be treated as a command. Empty = every
    /// message from an allowed user is a command.
    pub prefix: String,
    /// Safety mode for remote turns.
    pub safety_mode: String,
    /// VPS ids a remote turn may act on.
    pub targets: Vec<String>,
}

impl Config {
    /// Whether the bridge should run at all.
    ///
    /// Requires a channel *and* at least one allowed user. A configuration with no
    /// allowlist is not "open to everyone", it is not ready — refusing to start is
    /// the only safe reading.
    pub fn is_usable(&self) -> bool {
        self.enabled && !self.channel_id.trim().is_empty() && !self.allowed_user_ids.is_empty()
    }
}

/// Split a comma/space/newline separated settings value into ids.
pub fn parse_id_list(raw: &str) -> Vec<String> {
    raw.split([',', ' ', '\n', '\r', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// One inbound chat message, normalised across platforms.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncomingMessage {
    pub id: String,
    pub channel_id: String,
    pub author_id: String,
    pub author_name: String,
    pub is_bot: bool,
    pub content: String,
}

/// Why a message was not acted on. Carried rather than discarded so the reasons can
/// be surfaced in diagnostics — "the bot ignored me" is otherwise unanswerable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    Disabled,
    WrongChannel,
    FromBot,
    NotAllowed,
    NoPrefix,
    Empty,
}

/// Decide whether a message may drive the agent, and what it is asking.
///
/// Pure, so every branch of the security decision is testable without a network.
pub fn authorize(cfg: &Config, msg: &IncomingMessage) -> Result<String, Rejected> {
    if !cfg.is_usable() {
        return Err(Rejected::Disabled);
    }
    if msg.channel_id != cfg.channel_id {
        return Err(Rejected::WrongChannel);
    }
    // Covers our own replies, so the agent cannot end up in a conversation with
    // itself.
    if msg.is_bot {
        return Err(Rejected::FromBot);
    }
    if !cfg.allowed_user_ids.iter().any(|id| id == &msg.author_id) {
        return Err(Rejected::NotAllowed);
    }
    let body = match cfg.prefix.trim() {
        "" => msg.content.trim(),
        prefix => match msg.content.trim().strip_prefix(prefix) {
            Some(rest) => rest.trim(),
            None => return Err(Rejected::NoPrefix),
        },
    };
    if body.is_empty() {
        return Err(Rejected::Empty);
    }
    Ok(body.to_string())
}

/// Split a reply into platform-sized chunks, preferring line boundaries.
///
/// Discord rejects anything over 2000 characters outright, so an un-split reply is
/// not truncated — it is lost. Splitting on lines keeps code blocks and command
/// output readable rather than cutting mid-token.
pub fn chunk_reply(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        // A single line longer than the limit has to be hard-split; there is no
        // boundary to prefer.
        if line.len() > max {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let mut rest = line;
            while !rest.is_empty() {
                let cut = crate::ai::text::floor_char_boundary(rest, max.min(rest.len()));
                let cut = if cut == 0 { rest.len() } else { cut };
                out.push(rest[..cut].to_string());
                rest = &rest[cut..];
            }
            continue;
        }
        if cur.len() + line.len() > max {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|s| s.trim_end().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Chunk a reply for Discord specifically.
pub fn chunk_for_discord(text: &str) -> Vec<String> {
    chunk_reply(text, DISCORD_MAX_CHARS)
}

// ---------------------------------------------------------------------------
// Discord transport
// ---------------------------------------------------------------------------

/// Discord's REST base. Pinned to a version so a future default cannot change the
/// response shape underneath us.
const DISCORD_API: &str = "https://discord.com/api/v10";

/// Idle gap between polls.
///
/// Discord allows ~5 requests per 5 seconds on this route, so 3s is comfortably
/// inside the budget while still feeling immediate on a phone. It only matters while
/// the bridge is enabled — the loop exits entirely when it is not.
const POLL_IDLE: std::time::Duration = std::time::Duration::from_secs(3);

/// Backoff after a transport error, so a network drop or a revoked token does not
/// turn into a hot loop against Discord.
const POLL_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

/// Fetch messages newer than `after`, oldest first.
pub async fn fetch_messages(
    token: &str,
    channel_id: &str,
    after: Option<&str>,
) -> Result<Vec<IncomingMessage>, String> {
    let mut url = format!("{DISCORD_API}/channels/{channel_id}/messages?limit=20");
    if let Some(after) = after {
        url.push_str(&format!("&after={after}"));
    }
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bot {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        // Deliberately does not include the body: a Discord error can echo request
        // context, and this string reaches the log.
        return Err(format!("discord returned {}", resp.status()));
    }
    let raw: Vec<serde_json::Value> = resp.json().await.map_err(|e| e.to_string())?;
    // Discord returns newest first; the agent should read them in the order they
    // were sent, or a two-message instruction arrives backwards.
    Ok(raw
        .into_iter()
        .rev()
        .filter_map(|v| {
            Some(IncomingMessage {
                id: v.get("id")?.as_str()?.to_string(),
                channel_id: v.get("channel_id")?.as_str().unwrap_or(channel_id).to_string(),
                author_id: v.get("author")?.get("id")?.as_str()?.to_string(),
                author_name: v
                    .get("author")
                    .and_then(|a| a.get("username"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("someone")
                    .to_string(),
                is_bot: v
                    .get("author")
                    .and_then(|a| a.get("bot"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                content: v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string(),
            })
        })
        .collect())
}

/// Post a reply, split into platform-sized messages.
pub async fn send_message(token: &str, channel_id: &str, text: &str) -> Result<(), String> {
    for chunk in chunk_for_discord(text) {
        let resp = client()
            .post(format!("{DISCORD_API}/channels/{channel_id}/messages"))
            .header("Authorization", format!("Bot {token}"))
            .json(&serde_json::json!({ "content": chunk }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("discord rejected a reply: {}", resp.status()));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------

/// Read the bridge configuration out of settings.
pub fn load_config(db: &crate::storage::Db) -> Config {
    let get = |key: &str| db.get_setting(key).ok().flatten().unwrap_or_default();
    Config {
        enabled: get(SETTING_ENABLED) == "true",
        channel_id: get(SETTING_CHANNEL).trim().to_string(),
        allowed_user_ids: parse_id_list(&get(SETTING_ALLOWED)),
        prefix: get(SETTING_PREFIX),
        // Strictest by default. An approval prompt opens a modal on a desktop nobody
        // is sitting at, so a remote turn that needs one blocks until it times out —
        // better that than a phone silently authorising a destructive command.
        safety_mode: match get(SETTING_SAFETY).as_str() {
            m @ ("full" | "allowlist" | "approve") => m.to_string(),
            _ => "allowlist".to_string(),
        },
        targets: parse_id_list(&get(SETTING_TARGETS)),
    }
}

/// Poll the configured channel and run whatever an authorised user asks for.
///
/// Spawned once at startup. It costs nothing while disabled: with no usable config
/// it sleeps and re-reads settings, so enabling the bridge takes effect without a
/// restart and disabling it stops the traffic immediately.
pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let mut after: Option<String> = None;
        loop {
            tokio::time::sleep(POLL_IDLE).await;

            let db = app.state::<crate::storage::Db>().inner().clone();
            let cfg = load_config(&db);
            if !cfg.is_usable() {
                // Not configured, or deliberately off. Drop the cursor so re-enabling
                // does not replay a backlog of messages sent while it was off — those
                // were not addressed to a listening agent.
                after = None;
                continue;
            }
            let Some(token) = crate::secrets::get_secret(SECRET_DISCORD_TOKEN)
                .ok()
                .flatten()
                .filter(|t| !t.trim().is_empty())
            else {
                continue;
            };

            let messages = match fetch_messages(&token, &cfg.channel_id, after.as_deref()).await {
                Ok(m) => m,
                Err(e) => {
                    crate::diag(&format!("remote: poll failed: {e}"));
                    tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                    continue;
                }
            };

            for msg in messages {
                // Advance past every message we have seen, authorised or not —
                // otherwise one un-actionable message is re-fetched forever.
                after = Some(msg.id.clone());
                let ask = match authorize(&cfg, &msg) {
                    Ok(text) => text,
                    Err(Rejected::NotAllowed) => {
                        crate::diag(&format!(
                            "remote: refused a command from {} ({})",
                            msg.author_name, msg.author_id
                        ));
                        continue;
                    }
                    Err(_) => continue,
                };
                crate::diag(&format!("remote: running a command from {}", msg.author_name));
                let reply = run_remote_turn(&app, &cfg, &ask).await;
                if let Err(e) = send_message(&token, &cfg.channel_id, &reply).await {
                    crate::diag(&format!("remote: could not reply: {e}"));
                }
            }
        }
    });
}

/// Run one agent turn on behalf of a remote message and return what to send back.
async fn run_remote_turn(app: &tauri::AppHandle, cfg: &Config, ask: &str) -> String {
    use tauri::Manager;
    let db = app.state::<crate::storage::Db>().inner().clone();
    let hooks_cfg = if db.get_setting("agent.hooks_enabled").ok().flatten().as_deref()
        == Some("false")
    {
        crate::ai::hooks::HooksConfig::default()
    } else {
        crate::ai::hooks::HooksConfig::load(&app.state::<crate::ai::AgentHome>().inner().clone())
    };

    let tc = crate::ai::tools::ToolContext {
        app: app.clone(),
        db: db.clone(),
        sessions: app.state::<crate::ssh::SessionManager>().inner().clone(),
        home: app.state::<crate::ai::AgentHome>().inner().clone(),
        approvals: app.state::<crate::ai::safety::ApprovalRegistry>().inner().clone(),
        // Nobody is at the desktop to answer, so interactive prompts get a fresh
        // registry that nothing will resolve; ask_user times out rather than
        // silently proceeding.
        prompts: crate::ai::interaction::PromptRegistry::new(),
        session_state: crate::ai::interaction::SessionState::new(),
        // One session per remote request. A shared id would let a stranger's earlier
        // message shape a later one's context.
        session_id: format!("remote:{}", uuid::Uuid::new_v4()),
        targets: cfg.targets.clone(),
        safety: cfg.safety_mode.clone(),
        plan_mode: false,
        workspace_id: None,
        canvas: Vec::new(),
        edits: crate::ai::edits::EditJournal::with_db(db.clone()),
        hooks: hooks_cfg,
        turn_images: Vec::new(),
        goal_id: None,
    };

    let prompt = format!(
        "This request arrived over remote chat, not from the desktop app. The user is \
         on their phone: keep the reply short and plain-text (no wide tables, no long \
         file dumps). Nobody can answer an approval prompt right now, so if something \
         needs one, say what you would do and stop rather than waiting.\n\n{ask}"
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::ai::provider::StreamEvent>();
    // Nothing consumes the stream here — the reply is the final message — but the
    // sink has to be drained or the turn blocks on a full channel.
    let drain = tauri::async_runtime::spawn(async move { while rx.recv().await.is_some() {} });

    let result = crate::ai::agent::run_turn(
        &tc,
        None,
        vec![crate::ai::provider::ChatMessage::user(prompt)],
        false,
        &tx,
    )
    .await;
    drop(tx);
    let _ = drain.await;

    match result {
        Ok(msg) if !msg.content.trim().is_empty() => msg.content,
        Ok(_) => "(the agent finished without saying anything)".to_string(),
        Err(e) => format!("Failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            enabled: true,
            channel_id: "chan-1".into(),
            allowed_user_ids: vec!["user-1".into()],
            prefix: "!x".into(),
            safety_mode: "approve".into(),
            targets: vec![],
        }
    }

    fn msg() -> IncomingMessage {
        IncomingMessage {
            id: "m1".into(),
            channel_id: "chan-1".into(),
            author_id: "user-1".into(),
            author_name: "owner".into(),
            is_bot: false,
            content: "!x restart nginx".into(),
        }
    }

    #[test]
    fn an_allowed_user_in_the_right_channel_is_accepted() {
        assert_eq!(authorize(&cfg(), &msg()), Ok("restart nginx".into()));
    }

    #[test]
    fn an_empty_allowlist_authorises_nobody() {
        // The single most dangerous default to get wrong: "no list" must not mean
        // "everyone in the channel".
        let mut c = cfg();
        c.allowed_user_ids.clear();
        assert!(!c.is_usable());
        assert_eq!(authorize(&c, &msg()), Err(Rejected::Disabled));
    }

    #[test]
    fn a_stranger_in_the_right_channel_is_refused() {
        let mut m = msg();
        m.author_id = "someone-else".into();
        assert_eq!(authorize(&cfg(), &m), Err(Rejected::NotAllowed));
    }

    #[test]
    fn another_channel_grants_nothing() {
        // Being added to a second channel must not extend control to it.
        let mut m = msg();
        m.channel_id = "chan-2".into();
        assert_eq!(authorize(&cfg(), &m), Err(Rejected::WrongChannel));
    }

    #[test]
    fn bot_messages_are_ignored_including_our_own() {
        // Otherwise a reply containing the prefix drives the next turn and the agent
        // talks to itself until the tokens run out.
        let mut m = msg();
        m.is_bot = true;
        assert_eq!(authorize(&cfg(), &m), Err(Rejected::FromBot));
    }

    #[test]
    fn disabled_means_disabled_even_when_fully_configured() {
        let mut c = cfg();
        c.enabled = false;
        assert_eq!(authorize(&c, &msg()), Err(Rejected::Disabled));
    }

    #[test]
    fn the_prefix_gates_ordinary_chatter() {
        let mut m = msg();
        m.content = "morning everyone".into();
        assert_eq!(authorize(&cfg(), &m), Err(Rejected::NoPrefix));

        // No prefix configured: any message from an allowed user is a command.
        let mut c = cfg();
        c.prefix = "".into();
        assert_eq!(authorize(&c, &m), Ok("morning everyone".into()));
    }

    #[test]
    fn a_prefix_with_nothing_after_it_is_not_a_command() {
        let mut m = msg();
        m.content = "!x   ".into();
        assert_eq!(authorize(&cfg(), &m), Err(Rejected::Empty));
    }

    #[test]
    fn id_lists_accept_whatever_the_user_pasted() {
        assert_eq!(
            parse_id_list("111, 222\n333\t444 555"),
            vec!["111", "222", "333", "444", "555"]
        );
        assert!(parse_id_list("   ,, \n ").is_empty());
    }

    #[test]
    fn short_replies_are_sent_whole() {
        assert_eq!(chunk_reply("all good", 2000), vec!["all good"]);
        assert!(chunk_reply("   ", 2000).is_empty());
    }

    #[test]
    fn long_replies_split_on_line_boundaries() {
        let text = (0..50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let chunks = chunk_reply(&text, 60);
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(c.len() <= 60, "chunk too long: {}", c.len());
        }
        // Nothing is dropped: every line survives somewhere.
        let joined = chunks.join("\n");
        for i in 0..50 {
            assert!(joined.contains(&format!("line {i}")), "lost line {i}");
        }
    }

    #[test]
    fn a_single_overlong_line_is_hard_split_rather_than_dropped() {
        // Discord rejects >2000 outright, so an unsplit line is lost, not truncated.
        let text = "x".repeat(5000);
        let chunks = chunk_reply(&text, 2000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 5000);
    }

    #[test]
    fn splitting_never_cuts_a_codepoint() {
        // Command output is full of box drawing and accented paths.
        let text = "é".repeat(1000);
        let chunks = chunk_reply(&text, 999);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks.concat().chars().count(), 1000);
    }

    #[test]
    fn discord_chunks_respect_the_platform_limit() {
        let chunks = chunk_for_discord(&"y".repeat(4500));
        assert!(chunks.iter().all(|c| c.len() <= 2000));
        assert_eq!(chunks.concat().len(), 4500);
    }
}
