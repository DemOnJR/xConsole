//! Remote control: drive the agent from a chat app while xConsole is running.
//!
//! The point is to reach your servers from your phone without xConsole being
//! reachable *from* the internet. So every transport polls outbound over HTTPS and
//! never listens — no port, no tunnel, no inbound firewall rule. Close xConsole and
//! remote control stops existing, which is the intended failure mode.
//!
//! Three transports share this file's security decision and differ only in how bytes
//! move: [`discord`], [`telegram`], and [`whatsapp`]. They were added in that order
//! because that is the order of onboarding pain — Discord needs a developer-portal
//! app and a bot, Telegram needs one chat with BotFather, WhatsApp needs a QR scan.
//!
//! # Threat model
//!
//! This turns a chat message into commands on the user's infrastructure, so the
//! authorisation decision is the whole feature. Every rule here fails closed:
//!
//! - Off unless explicitly enabled, per transport as well as globally.
//! - An empty allowlist authorises **nobody**. "No list configured" must never mean
//!   "anyone who can reach the bot", which is the mistake that would hand a server to
//!   whoever wanders into a Discord or DMs a Telegram bot.
//! - Where a chat is configured, only that one is read, so being added to another
//!   grants nothing.
//! - Bot messages and our own replies are ignored — otherwise a reply that happens to
//!   contain the prefix drives the next turn, and the agent talks to itself until the
//!   tokens run out.
//! - The safety mode still applies. Nobody can answer an approval prompt from a
//!   phone, so a remote turn runs under its own mode and defaults to the strictest.

pub mod discord;
pub mod telegram;
pub mod whatsapp;

use serde::{Deserialize, Serialize};

/// Settings keys shared by every transport.
pub const SETTING_ENABLED: &str = "remote.enabled";
pub const SETTING_PREFIX: &str = "remote.prefix";
pub const SETTING_SAFETY: &str = "remote.safety_mode";
pub const SETTING_TARGETS: &str = "remote.targets";

/// Legacy aliases, kept so the existing Discord configuration keeps working without a
/// migration. New transports use the `chat_id` spelling.
pub const SETTING_CHANNEL: &str = "remote.discord.channel_id";
pub const SETTING_ALLOWED: &str = "remote.discord.allowed_user_ids";
pub const SECRET_DISCORD_TOKEN: &str = "remote.discord.token";

/// Which chat platform a bridge speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Discord,
    Telegram,
    WhatsApp,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Discord, Kind::Telegram, Kind::WhatsApp];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Discord => "discord",
            Kind::Telegram => "telegram",
            Kind::WhatsApp => "whatsapp",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "discord" => Some(Kind::Discord),
            "telegram" => Some(Kind::Telegram),
            "whatsapp" | "wa" => Some(Kind::WhatsApp),
            _ => None,
        }
    }

    /// Longest reply one message on this platform can carry.
    ///
    /// Discord's 2000 is a hard API limit — an over-long message is rejected, not
    /// truncated. Telegram's is 4096. WhatsApp accepts far more, but a wall of text on
    /// a phone is unreadable, so it borrows Telegram's number.
    pub fn max_chars(self) -> usize {
        match self {
            Kind::Discord => 2000,
            Kind::Telegram | Kind::WhatsApp => 4096,
        }
    }

    /// Whether a configured chat id is mandatory.
    ///
    /// A Discord bot sits in a guild and can see many channels, so naming one is the
    /// difference between "reads my ops channel" and "reads everything". Telegram and
    /// WhatsApp bridges are reached by direct message, where the allowlist already
    /// answers "who", and a chat id is an optional extra narrowing.
    pub fn chat_required(self) -> bool {
        matches!(self, Kind::Discord)
    }

    /// Setting key for this transport's own on/off switch.
    pub fn setting_enabled(self) -> String {
        format!("remote.{}.enabled", self.as_str())
    }

    /// Setting key for the chat this transport reads.
    pub fn setting_chat(self) -> String {
        match self {
            // Spelled `channel_id` from before there were other transports. Renaming it
            // would silently disarm every existing install.
            Kind::Discord => SETTING_CHANNEL.to_string(),
            _ => format!("remote.{}.chat_id", self.as_str()),
        }
    }

    /// Setting key for this transport's allowlist.
    pub fn setting_allowed(self) -> String {
        match self {
            Kind::Discord => SETTING_ALLOWED.to_string(),
            _ => format!("remote.{}.allowed_user_ids", self.as_str()),
        }
    }

    /// Keychain entry holding this transport's credential.
    ///
    /// WhatsApp has none: it authenticates by a paired device session the sidecar
    /// holds on disk, which is the whole reason its onboarding is a QR scan rather
    /// than a token paste.
    pub fn secret_key(self) -> Option<String> {
        match self {
            Kind::WhatsApp => None,
            Kind::Discord => Some(SECRET_DISCORD_TOKEN.to_string()),
            _ => Some(format!("remote.{}.token", self.as_str())),
        }
    }
}

/// How one transport's bridge is configured. Built from settings; see
/// [`Config::is_usable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub kind: Kind,
    /// The global switch and this transport's own switch, already combined.
    pub enabled: bool,
    /// Chat this bridge reads. Empty means "wherever the allowlist reaches me", which
    /// only some platforms permit — see [`Kind::chat_required`].
    pub chat_id: String,
    /// Chat-platform identities permitted to command the agent. Entries may be ids,
    /// phone numbers, or `@usernames`; see [`allow_matches`].
    pub allowed_user_ids: Vec<String>,
    /// Messages must start with this to be treated as a command. Empty = every
    /// message from an allowed user is a command.
    pub prefix: String,
    /// Safety mode for remote turns.
    pub safety_mode: String,
    /// VPS ids a remote turn may act on.
    pub targets: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            kind: Kind::Discord,
            enabled: false,
            chat_id: String::new(),
            allowed_user_ids: Vec::new(),
            prefix: String::new(),
            safety_mode: "allowlist".into(),
            targets: Vec::new(),
        }
    }
}

impl Config {
    /// Whether the bridge should run at all.
    ///
    /// Requires at least one allowed user, and a chat where the platform needs one. A
    /// configuration with no allowlist is not "open to everyone", it is not ready —
    /// refusing to start is the only safe reading.
    pub fn is_usable(&self) -> bool {
        self.enabled
            && !self.allowed_user_ids.is_empty()
            && (!self.kind.chat_required() || !self.chat_id.trim().is_empty())
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

/// Who sent a message, normalised across platforms.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Author {
    /// The platform's stable id: a Discord snowflake, a Telegram numeric id, or the
    /// phone number behind a WhatsApp JID.
    pub id: String,
    /// The handle, where the platform has one. Telegram has always had `@name`;
    /// WhatsApp added usernames, so a person can now be addressed without their phone
    /// number ever being written down in a settings field.
    #[serde(default)]
    pub username: Option<String>,
    pub display_name: String,
}

/// One inbound chat message, normalised across platforms.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncomingMessage {
    pub id: String,
    pub chat_id: String,
    pub author: Author,
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

/// Keep only the digits, so the same phone number written five ways matches itself.
///
/// A WhatsApp allowlist is typed by a human from their contacts, where the number may
/// carry `+`, spaces, dashes or parentheses. Comparing the raw strings would reject
/// the owner from their own bridge, and the natural workaround — telling people to
/// paste a raw JID — puts the least readable form of the most security-critical
/// setting in front of them.
fn digits(s: &str) -> String {
    let d: String = s.chars().filter(char::is_ascii_digit).collect();
    // `00` is the international access code — the written-out form of `+`, and the one
    // most phones store. Folding it away lets `+40…`, `0040…` and `40…` all mean the
    // same person.
    //
    // The *national* trunk `0` (`0712…`) is deliberately left alone: without knowing
    // the country there is no way to expand it, and guessing would let one allowlist
    // entry match a stranger in another country who happens to share the digits.
    d.strip_prefix("00").map(str::to_string).unwrap_or(d)
}

/// Does one allowlist entry authorise this author?
///
/// Three spellings are accepted, disambiguated by shape rather than by trying each in
/// turn, so that one kind of identity can never be silently satisfied by another:
///
/// - `@handle` — a username, and *only* a username. Explicit, so a person who means
///   the handle cannot accidentally match an id.
/// - all digits — an id or phone number, compared digit-wise.
/// - anything else — a bare username, matched case-insensitively.
///
/// The `@` form matters most on WhatsApp, where the alternative is writing a personal
/// phone number into a config file.
pub fn allow_matches(entry: &str, author: &Author) -> bool {
    let entry = entry.trim();
    if entry.is_empty() {
        return false;
    }
    let username_matches = |name: &str| {
        author
            .username
            .as_deref()
            .is_some_and(|u| u.trim_start_matches('@').eq_ignore_ascii_case(name))
    };

    if let Some(handle) = entry.strip_prefix('@') {
        return !handle.is_empty() && username_matches(handle);
    }
    if entry.chars().all(|c| c.is_ascii_digit() || " -()+.".contains(c)) {
        let want = digits(entry);
        return !want.is_empty() && want == digits(&author.id);
    }
    entry.eq_ignore_ascii_case(&author.id) || username_matches(entry)
}

/// Decide whether a message may drive the agent, and what it is asking.
///
/// Pure, so every branch of the security decision is testable without a network.
pub fn authorize(cfg: &Config, msg: &IncomingMessage) -> Result<String, Rejected> {
    if !cfg.is_usable() {
        return Err(Rejected::Disabled);
    }
    // An unset chat id is only reachable on platforms where the allowlist is the whole
    // boundary; `is_usable` has already refused it everywhere else.
    if !cfg.chat_id.trim().is_empty() && msg.chat_id != cfg.chat_id.trim() {
        return Err(Rejected::WrongChannel);
    }
    // Covers our own replies, so the agent cannot end up in a conversation with
    // itself.
    if msg.is_bot {
        return Err(Rejected::FromBot);
    }
    if !cfg.allowed_user_ids.iter().any(|e| allow_matches(e, &msg.author)) {
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

/// Chunk a reply for one platform's message limit.
pub fn chunk_for(kind: Kind, text: &str) -> Vec<String> {
    chunk_reply(text, kind.max_chars())
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Read one transport's configuration out of settings.
pub fn load_config(db: &crate::storage::Db, kind: Kind) -> Config {
    let get = |key: &str| db.get_setting(key).ok().flatten().unwrap_or_default();
    let master = get(SETTING_ENABLED) == "true";
    // Discord predates the per-transport switch, so a missing key means "on" for it
    // and "off" for everyone else. Reading it the other way would disarm every
    // install that upgrades into this version.
    let own = match db.get_setting(&kind.setting_enabled()).ok().flatten() {
        Some(v) => v == "true",
        None => kind == Kind::Discord,
    };
    Config {
        kind,
        enabled: master && own,
        chat_id: get(&kind.setting_chat()).trim().to_string(),
        allowed_user_ids: parse_id_list(&get(&kind.setting_allowed())),
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

/// Fetch a transport's credential, if it has one and it is set.
pub fn load_token(kind: Kind) -> Option<String> {
    let key = kind.secret_key()?;
    crate::secrets::get_secret(&key)
        .ok()
        .flatten()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

// ---------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------

/// Idle gap between polls.
///
/// Discord allows ~5 requests per 5 seconds on its messages route, so 3s is
/// comfortably inside the budget while still feeling immediate on a phone. It only
/// matters while a bridge is enabled — each loop idles cheaply when it is not.
const POLL_IDLE: std::time::Duration = std::time::Duration::from_secs(3);

/// Backoff after a transport error, so a network drop or a revoked token does not
/// turn into a hot loop against the platform.
const POLL_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

/// One remote turn at a time, across every transport.
///
/// Without this, a message arriving on Telegram while a Discord turn is mid-migration
/// would start a second agent on the same servers. Two turns racing on one box is a
/// worse failure than a few seconds of queueing, and the queueing is invisible from a
/// phone.
static TURN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What a transport must do to carry the bridge.
///
/// Implementors own their own cursor: Discord pages by message id, Telegram by update
/// offset, WhatsApp by a live event stream from its sidecar. Keeping that private is
/// what lets the driver below hold the single copy of the security decision.
#[async_trait::async_trait]
pub trait Transport: Send {
    fn kind(&self) -> Kind;

    /// Fetch messages that arrived since the last call, oldest first.
    async fn poll(&mut self, cfg: &Config) -> Result<Vec<IncomingMessage>, String>;

    /// Reply where the message came from, not where the config points — a bridge with
    /// no configured chat still has to answer somebody.
    async fn send(&mut self, cfg: &Config, to: &IncomingMessage, text: &str) -> Result<(), String>;

    /// Drop cursors and connections; the bridge is off or unconfigured.
    fn reset(&mut self) {}
}

/// Start every transport's bridge. Each runs independently, so a broken Discord token
/// does not stop Telegram.
pub fn spawn(app: tauri::AppHandle) {
    for kind in Kind::ALL {
        let transport: Box<dyn Transport> = match kind {
            Kind::Discord => Box::new(discord::Discord::default()),
            Kind::Telegram => Box::new(telegram::Telegram::default()),
            Kind::WhatsApp => Box::new(whatsapp::WhatsApp::new(app.clone())),
        };
        drive(app.clone(), transport);
    }
}

/// Poll one transport and run whatever an authorised user asks for.
///
/// Spawned once at startup. It costs nothing while disabled: with no usable config it
/// sleeps and re-reads settings, so enabling a bridge takes effect without a restart
/// and disabling it stops the traffic immediately.
fn drive(app: tauri::AppHandle, mut transport: Box<dyn Transport>) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let kind = transport.kind();
        loop {
            tokio::time::sleep(POLL_IDLE).await;

            let db = app.state::<crate::storage::Db>().inner().clone();
            let cfg = load_config(&db, kind);
            if !cfg.is_usable() {
                // Not configured, or deliberately off. Drop the cursor so re-enabling
                // does not replay a backlog of messages sent while it was off — those
                // were not addressed to a listening agent.
                transport.reset();
                continue;
            }

            let messages = match transport.poll(&cfg).await {
                Ok(m) => m,
                Err(e) => {
                    crate::diag(&format!("remote({}): poll failed: {e}", kind.as_str()));
                    tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                    continue;
                }
            };

            for msg in messages {
                let ask = match authorize(&cfg, &msg) {
                    Ok(text) => text,
                    Err(Rejected::NotAllowed) => {
                        crate::diag(&format!(
                            "remote({}): refused a command from {} ({})",
                            kind.as_str(),
                            msg.author.display_name,
                            msg.author.id
                        ));
                        continue;
                    }
                    Err(_) => continue,
                };
                crate::diag(&format!(
                    "remote({}): running a command from {}",
                    kind.as_str(),
                    msg.author.display_name
                ));
                let reply = {
                    let _turn = TURN_LOCK.lock().await;
                    run_remote_turn(&app, &cfg, &ask).await
                };
                if let Err(e) = transport.send(&cfg, &msg, &reply).await {
                    crate::diag(&format!("remote({}): could not reply: {e}", kind.as_str()));
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
            kind: Kind::Discord,
            enabled: true,
            chat_id: "chan-1".into(),
            allowed_user_ids: vec!["user-1".into()],
            prefix: "!x".into(),
            safety_mode: "approve".into(),
            targets: vec![],
        }
    }

    fn msg() -> IncomingMessage {
        IncomingMessage {
            id: "m1".into(),
            chat_id: "chan-1".into(),
            author: Author {
                id: "user-1".into(),
                username: None,
                display_name: "owner".into(),
            },
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
        // And that holds for the transports that do not require a chat id either, so
        // an unconfigured Telegram bot cannot be commanded by whoever DMs it first.
        for kind in [Kind::Telegram, Kind::WhatsApp] {
            let mut c = cfg();
            c.kind = kind;
            c.chat_id = String::new();
            c.allowed_user_ids.clear();
            assert!(!c.is_usable(), "{} armed with no allowlist", kind.as_str());
        }
    }

    #[test]
    fn a_stranger_in_the_right_channel_is_refused() {
        let mut m = msg();
        m.author.id = "someone-else".into();
        assert_eq!(authorize(&cfg(), &m), Err(Rejected::NotAllowed));
    }

    #[test]
    fn another_channel_grants_nothing() {
        // Being added to a second channel must not extend control to it.
        let mut m = msg();
        m.chat_id = "chan-2".into();
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
    fn a_phone_number_matches_however_it_was_typed() {
        // The owner types their number out of their contacts app, punctuation and all;
        // WhatsApp reports bare digits. Rejecting them from their own bridge because
        // of a space would be indefensible.
        let author = Author {
            id: "40712345678".into(),
            username: None,
            display_name: "owner".into(),
        };
        for entry in ["+40 712 345 678", "0040712345678", "40-712-345-678", "40712345678"] {
            assert!(allow_matches(entry, &author), "did not match {entry}");
        }
        assert!(!allow_matches("40712345679", &author));
        // The national form carries no country, so it is not treated as the same
        // number. Matching it would mean one entry authorising a stranger abroad who
        // happens to share the digits.
        assert!(!allow_matches("0712345678", &author));
    }

    #[test]
    fn a_username_authorises_without_writing_down_a_phone_number() {
        // WhatsApp usernames exist precisely so a number need not be shared; the
        // allowlist has to be usable the same way.
        let author = Author {
            id: "40712345678".into(),
            username: Some("ada.lovelace".into()),
            display_name: "Ada".into(),
        };
        assert!(allow_matches("@ada.lovelace", &author));
        assert!(allow_matches("Ada.Lovelace", &author));
        assert!(!allow_matches("@someone.else", &author));
    }

    #[test]
    fn an_at_entry_never_falls_back_to_matching_an_id() {
        // Shape decides which kind of identity an entry is. If `@1234` could also match
        // an id, a person who meant a handle would be authorising a stranger's account
        // number without ever seeing that they had.
        let author = Author {
            id: "1234".into(),
            username: None,
            display_name: "n".into(),
        };
        assert!(!allow_matches("@1234", &author));
        assert!(allow_matches("1234", &author));
    }

    #[test]
    fn a_username_cannot_be_satisfied_by_someone_elses_number() {
        let author = Author {
            id: "40712345678".into(),
            username: Some("40712345678".into()),
            display_name: "n".into(),
        };
        // Digit-shaped entries are compared against the id only, so a handle made of
        // digits cannot stand in for the number.
        let other = Author {
            id: "999".into(),
            username: Some("40712345678".into()),
            display_name: "n".into(),
        };
        assert!(allow_matches("40712345678", &author));
        assert!(!allow_matches("40712345678", &other));
    }

    #[test]
    fn an_empty_allowlist_entry_matches_nobody() {
        let author = Author::default();
        assert!(!allow_matches("", &author));
        assert!(!allow_matches("   ", &author));
        assert!(!allow_matches("@", &author));
        // An author with no username is not matched by a bare handle either.
        assert!(!allow_matches("@ada", &author));
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
    fn every_platform_gets_chunks_it_will_accept() {
        for kind in Kind::ALL {
            let chunks = chunk_for(kind, &"y".repeat(9000));
            assert!(chunks.iter().all(|c| c.len() <= kind.max_chars()));
            assert_eq!(chunks.concat().len(), 9000);
        }
    }

    #[test]
    fn only_discord_keeps_the_legacy_settings_spelling() {
        // Renaming it would silently disarm every install that upgrades.
        assert_eq!(Kind::Discord.setting_chat(), SETTING_CHANNEL);
        assert_eq!(Kind::Discord.setting_allowed(), SETTING_ALLOWED);
        assert_eq!(Kind::Discord.secret_key().as_deref(), Some(SECRET_DISCORD_TOKEN));
        assert_eq!(Kind::Telegram.setting_chat(), "remote.telegram.chat_id");
        // WhatsApp authenticates by a paired session, not a pasted credential.
        assert_eq!(Kind::WhatsApp.secret_key(), None);
    }

    #[test]
    fn a_transport_that_needs_no_chat_still_arms_without_one() {
        // Telegram and WhatsApp are reached by DM, where the allowlist is the boundary.
        let mut c = cfg();
        c.kind = Kind::Telegram;
        c.chat_id = String::new();
        assert!(c.is_usable());
        let mut m = msg();
        m.chat_id = "whatever".into();
        assert_eq!(authorize(&c, &m), Ok("restart nginx".into()));

        // Discord still refuses, because its bot can see a whole guild.
        let mut d = cfg();
        d.chat_id = String::new();
        assert!(!d.is_usable());
    }
}
