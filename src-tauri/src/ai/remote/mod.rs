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
//! - The safety mode still applies, and a remote turn runs under its own mode, which
//!   defaults to the strictest. What needs approval is asked in the chat and answered
//!   there — with the command and its blast radius in front of the user — rather than
//!   opening a modal on a desktop nobody is sitting at. The allowlist is still the whole
//!   boundary: anyone who can send a command can also say yes to one.
//!
//! # One conversation, many transports
//!
//! All three bridges run at once and share a single thread ([`CONVERSATION_ID`]), so a
//! follow-up makes sense wherever it is typed: ask about nginx on Telegram, say "restart
//! it" on WhatsApp, and "it" still refers to nginx. That is a deliberate loosening of the
//! per-message isolation this used to have — everyone on an allowlist now shares context.
//! It is a far smaller grant than the one they already hold, which is arbitrary commands
//! on the user's servers; the allowlist remains the whole security boundary.
//!
//! Replies go back where the message came from. The exception is when the user asks to
//! move — "carry on over WhatsApp" — which the agent performs by calling `remote_reply_on`
//! rather than by any command syntax, so it reads as ordinary conversation.

pub mod discord;
pub mod reaction;
pub mod telegram;
pub mod whatsapp;

use serde::{Deserialize, Serialize};

/// Settings keys shared by every transport.
pub const SETTING_ENABLED: &str = "remote.enabled";
pub const SETTING_PREFIX: &str = "remote.prefix";
pub const SETTING_SAFETY: &str = "remote.safety_mode";
pub const SETTING_TARGETS: &str = "remote.targets";
/// Which named agent answers. Empty = the unnamed main agent.
pub const SETTING_PERSONA: &str = "remote.persona_id";

/// The one thread every transport shares. A fixed id, so it survives restarts and is
/// visible in the desktop conversation list like any other.
pub const CONVERSATION_ID: &str = "remote:conversation";

/// The chat the agent spoke in last, as `<kind>:<chat id>`. This is what "answer where we
/// last talked" resolves to when nothing else says otherwise.
pub const SETTING_LAST_ROUTE: &str = "remote.last_route";

/// How many messages of history ride into a remote turn.
///
/// Enough that a conversation holds together over an afternoon, bounded so a thread left
/// running for a month does not silently grow every request. Trimming from the front
/// keeps the recent turns, which are the ones a follow-up refers to.
const HISTORY_LIMIT: usize = 40;

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

    /// Setting key for the last chat we actually exchanged messages in on this
    /// transport.
    ///
    /// Distinct from [`Kind::setting_chat`], which is the chat the user *restricted* the
    /// bridge to and is often blank. Moving a conversation needs somewhere concrete to
    /// write, and "wherever we last spoke on WhatsApp" is the honest answer.
    pub fn setting_last_chat(self) -> String {
        format!("remote.{}.last_chat", self.as_str())
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
    /// The named agent that answers, if any.
    ///
    /// Without one, a message from a phone runs as the unnamed main agent — which then
    /// relays to whichever named agent the work belongs to, so the person on WhatsApp is
    /// talking to a middleman rather than to the agent they set up to lead the team.
    pub persona_id: Option<String>,
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
            persona_id: None,
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
    if !cfg.chat_id.trim().is_empty() {
        let want = cfg.chat_id.trim();
        let got = msg.chat_id.trim();
        let matches_chat = got == want
            || got.strip_suffix("@g.us").unwrap_or(got) == want.strip_suffix("@g.us").unwrap_or(want)
            || got.strip_suffix("@s.whatsapp.net").unwrap_or(got) == want.strip_suffix("@s.whatsapp.net").unwrap_or(want)
            || got.strip_suffix("@lid").unwrap_or(got) == want.strip_suffix("@lid").unwrap_or(want);
        if !matches_chat {
            return Err(Rejected::WrongChannel);
        }
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

/// Marks a message as the agent's rather than a person's.
///
/// On WhatsApp especially, the bridge replies from the user's *own* account, so its
/// messages sit in the same column, in the same bubble colour, as everything they typed
/// themselves. Scrolling back, there is nothing to tell "restart nginx" from the answer
/// to it. A prefix is the only thing that survives every client, every notification
/// preview and every quoted reply.
pub const AGENT_PREFIX: &str = "[xConsole]";

/// Put the marker on a reply, once.
///
/// Idempotent because a reply can be re-sent — moved to another platform, retried after
/// a failed send — and two prefixes read as a bug.
pub fn mark_as_agent(text: &str) -> String {
    let text = text.trim_start();
    if text.starts_with(AGENT_PREFIX) {
        return text.to_string();
    }
    format!("{AGENT_PREFIX} {text}")
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

/// Chunk a reply and mark every part as the agent's.
///
/// Every part, not only the first. A long answer arrives as several messages, and on
/// WhatsApp they all come from the user's own account — so the second and third would be
/// indistinguishable from something the user typed, which is exactly the confusion the
/// marker exists to remove.
///
/// The prefix comes out of the budget rather than being added afterwards: Discord
/// rejects anything over its limit outright, so a chunk that fits and then grows by
/// eleven characters is a message that never arrives.
pub fn agent_chunks(kind: Kind, text: &str) -> Vec<String> {
    let budget = kind.max_chars().saturating_sub(AGENT_PREFIX.len() + 1).max(1);
    chunk_reply(text, budget)
        .into_iter()
        .map(|c| mark_as_agent(&c))
        .collect()
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
    let mut allowed = parse_id_list(&get(&kind.setting_allowed()));
    // For WhatsApp, if the user paired a phone by QR scan but left the allowlist blank,
    // the paired phone and paired LID are the hardware-authenticated device owner.
    if kind == Kind::WhatsApp && allowed.is_empty() {
        if let Ok(Some(phone)) = db.get_setting("remote.whatsapp.paired_phone") {
            if !phone.trim().is_empty() {
                allowed.push(phone.trim().to_string());
            }
        }
        if let Ok(Some(lid)) = db.get_setting("remote.whatsapp.paired_lid") {
            if !lid.trim().is_empty() {
                allowed.push(lid.trim().to_string());
            }
        }
    }

    Config {
        kind,
        enabled: master && own,
        chat_id: get(&kind.setting_chat()).trim().to_string(),
        allowed_user_ids: allowed,
        prefix: get(SETTING_PREFIX),
        // Strictest by default. Approvals reach the chat rather than the desktop, so a
        // remote turn no longer stalls on a modal — but a mode that asks is still the
        // right default, because "ask" is what puts the command in front of a person
        // before it runs.
        safety_mode: match get(SETTING_SAFETY).as_str() {
            m @ ("full" | "allowlist" | "approve") => m.to_string(),
            _ => "allowlist".to_string(),
        },
        targets: parse_id_list(&get(SETTING_TARGETS)),
        persona_id: Some(get(SETTING_PERSONA)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    }
}

/// Fetch a transport's credential, if it has one and it is set.
/// Where a message can be sent: a transport and a chat on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub kind: Kind,
    pub chat_id: String,
}

impl Route {
    /// `<kind>:<chat id>`. Decoding splits at the *first* colon only, so a chat id
    /// containing one — a WhatsApp JID, say — survives the round trip intact.
    pub fn encode(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.chat_id)
    }

    pub fn decode(raw: &str) -> Option<Route> {
        let (kind, chat_id) = raw.split_once(':')?;
        let kind = Kind::parse(kind)?;
        (!chat_id.is_empty()).then(|| Route {
            kind,
            chat_id: chat_id.to_string(),
        })
    }
}

/// Remember where we just spoke, both globally and for this transport.
///
/// Two records because they answer different questions: "where was the last thing said"
/// (for an unprompted message) and "where do I write if the user asks to move to
/// WhatsApp" (for a transport that may not be the most recent one).
pub fn remember_route(db: &crate::storage::Db, route: &Route) {
    let _ = db.set_setting(SETTING_LAST_ROUTE, &route.encode());
    let _ = db.set_setting(&route.kind.setting_last_chat(), &route.chat_id);
}

/// The chat the agent spoke in last, if any.
pub fn last_route(db: &crate::storage::Db) -> Option<Route> {
    db.get_setting(SETTING_LAST_ROUTE)
        .ok()
        .flatten()
        .as_deref()
        .and_then(Route::decode)
}

/// Where to write on `kind`: the last chat we used there, else the one the bridge is
/// restricted to. `None` when we have never spoken there and nothing is configured —
/// which is a thing to say out loud, not to paper over by guessing.
pub fn route_for(db: &crate::storage::Db, kind: Kind) -> Option<Route> {
    let last = db
        .get_setting(&kind.setting_last_chat())
        .ok()
        .flatten()
        .filter(|c| !c.trim().is_empty());
    let configured = || load_config(db, kind).chat_id;
    let chat_id = last.or_else(|| {
        let c = configured();
        (!c.trim().is_empty()).then_some(c)
    })?;
    Some(Route { kind, chat_id })
}

/// Send to any transport without owning its driver.
///
/// The driver replies through the `Transport` it holds; this is for everything else — a
/// conversation moved to another platform, and a message the agent sends unprompted.
pub async fn send_to(route: &Route, text: &str) -> Result<(), String> {
    match route.kind {
        Kind::Discord => {
            let token = load_token(Kind::Discord).ok_or("no Discord token saved")?;
            discord::send_message(&token, &route.chat_id, text).await
        }
        Kind::Telegram => {
            let token = load_token(Kind::Telegram).ok_or("no Telegram token saved")?;
            telegram::send_message(&token, &route.chat_id, text).await
        }
        Kind::WhatsApp => whatsapp::send_message(&route.chat_id, text).await,
    }
}

/// The shared thread, oldest first.
pub fn load_history(db: &crate::storage::Db) -> Vec<crate::ai::provider::ChatMessage> {
    db.get_agent_conversation(CONVERSATION_ID)
        .ok()
        .flatten()
        .and_then(|c| serde_json::from_str(&c.messages_json).ok())
        .unwrap_or_default()
}

/// Replace the shared thread, keeping only the tail that fits [`HISTORY_LIMIT`].
pub fn save_history(db: &crate::storage::Db, messages: &[crate::ai::provider::ChatMessage]) {
    let tail = if messages.len() > HISTORY_LIMIT {
        &messages[messages.len() - HISTORY_LIMIT..]
    } else {
        messages
    };
    let Ok(messages_json) = serde_json::to_string(tail) else {
        return;
    };
    let _ = db.upsert_agent_conversation(&crate::storage::models::AgentConversationInput {
        id: CONVERSATION_ID.to_string(),
        title: Some("Remote chat".to_string()),
        targets: Vec::new(),
        messages_json,
    });
}

pub fn load_token(kind: Kind) -> Option<String> {
    let key = kind.secret_key()?;
    crate::secrets::get_secret(&key)
        .ok()
        .flatten()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

// ---------------------------------------------------------------------------
// Approvals, answered from the phone
// ---------------------------------------------------------------------------

/// How long a command stays answerable after the agent asked about it.
///
/// Long enough to survive a meeting, short enough that "yes" typed tomorrow morning
/// does not run something proposed in a conversation nobody remembers.
const PENDING_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// How long a yes stays usable. It only has to cover the agent re-issuing the same
/// command on the next turn, which happens in seconds.
const GRANT_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// A command a remote turn wanted to run, waiting for a person to say yes.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    /// The exact string the agent proposed. A grant matches on this and nothing else.
    pub command: String,
    /// What the user is shown: redacted, and carrying the blast radius when there is one.
    pub shown: String,
    /// What the reviewing agent said about it, when there was one to ask. The user
    /// answers better for knowing somebody has already looked.
    pub reviewed: Option<String>,
    asked_at: std::time::Instant,
}

/// Where a command that needed a person ended up, for the reply to report.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Put to the user, waiting on their yes or no.
    Waiting(PendingApproval),
    /// Stopped by the reviewing agent, so the user was never asked to decide it. Still
    /// reported: an agent that quietly drops work is worse than one that argues about it.
    Vetoed { line: String, shown: String },
}

/// What a turn driven from a chat knows about itself.
#[derive(Debug, Clone)]
struct TurnContext {
    /// The agent answering, if a persona was configured for the bridge.
    persona_id: Option<String>,
    /// The message that started the turn, which is what makes a destructive command
    /// readable as either the job or a mistake.
    ask: String,
}

/// Set while a turn started by a chat message is running.
///
/// The desktop can be sitting on the same conversation, so the session id alone does not
/// say where the user is. This does. A `std::sync::Mutex` rather than an async one
/// because the guard clears it on drop, where there is nothing to await with.
static TURN_CTX: std::sync::Mutex<Option<TurnContext>> = std::sync::Mutex::new(None);

/// The command waiting on an answer, if any. One at a time: a second question replaces
/// the first, because a phone cannot keep two of these straight either.
static OUTCOME: tokio::sync::Mutex<Option<Outcome>> = tokio::sync::Mutex::const_new(None);

/// A yes, spent the first time the same command comes back.
static GRANT: tokio::sync::Mutex<Option<(String, std::time::Instant)>> =
    tokio::sync::Mutex::const_new(None);

/// Marks a remote turn as running, and unmarks it however the turn ends.
pub struct RemoteTurnGuard;

impl RemoteTurnGuard {
    pub fn enter(persona_id: Option<String>, ask: &str) -> Self {
        if let Ok(mut ctx) = TURN_CTX.lock() {
            *ctx = Some(TurnContext { persona_id, ask: ask.to_string() });
        }
        RemoteTurnGuard
    }
}

impl Drop for RemoteTurnGuard {
    fn drop(&mut self) {
        if let Ok(mut ctx) = TURN_CTX.lock() {
            *ctx = None;
        }
    }
}

fn turn_context() -> Option<TurnContext> {
    TURN_CTX.lock().ok().and_then(|c| c.clone())
}

pub fn turn_in_flight() -> bool {
    turn_context().is_some()
}

/// What a message did to the question the agent was waiting on.
#[derive(Debug, Clone)]
pub enum Answer {
    Approved(PendingApproval),
    Denied(PendingApproval),
    /// No question was open, or this message was about something else.
    Unrelated,
}

/// Yes or no, in the words people type on a phone — or neither, which is the answer
/// that matters most.
///
/// Every word has to be part of the answer. `"nu merge site-ul"` opens with a no and is
/// not one: it is the next job, and reading it as a refusal would throw the message away
/// and leave the user waiting on an agent that thinks it was told to stop.
pub fn verdict(text: &str) -> Option<bool> {
    const YES: &[&str] = &[
        "da", "ok", "okay", "k", "yes", "yep", "yeah", "y", "sure", "sigur", "hai",
        "ruleaza", "rulează", "run", "go", "aproba", "aprobă", "aprob", "fa", "fă",
        "fa-o", "fă-o", "da-i", "dă-i", "👍", "✅", "👌",
    ];
    const NO: &[&str] = &[
        "nu", "no", "n", "nope", "nah", "stop", "anuleaza", "anulează", "renunta",
        "renunță", "lasa", "lasă", "skip", "👎", "❌", "✋",
    ];
    // Words that carry no decision but are typed alongside one.
    const FILLER: &[&str] = &[
        "te", "rog", "please", "acum", "now", "it", "o", "il", "îl", "la", "asta",
        "this", "mersi", "thanks", "ty",
    ];

    let cleaned = text
        .to_lowercase()
        .replace(['.', ',', '!', '?', ';', ':', '"'], " ");
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.is_empty() || words.len() > 3 {
        return None;
    }
    let known = |w: &str| YES.contains(&w) || NO.contains(&w) || FILLER.contains(&w);
    if !words.iter().all(|w| known(w)) {
        return None;
    }
    // A no anywhere wins: "da... nu" is not a yes, and neither is anything ambiguous.
    if words.iter().any(|w| NO.contains(w)) {
        return Some(false);
    }
    words.iter().any(|w| YES.contains(w)).then_some(true)
}

/// Take an approval decision out of an incoming message, if there was a question open.
///
/// Called before the message becomes a turn, so a bare "da" resolves the command rather
/// than reaching the agent as a new instruction it cannot make sense of.
pub async fn answer_pending(text: &str) -> Answer {
    let mut slot = OUTCOME.lock().await;
    let pending = match slot.clone() {
        Some(Outcome::Waiting(p)) if p.asked_at.elapsed() <= PENDING_TTL => p,
        // Nothing open, open too long to still be about this conversation, or a refusal
        // that was reported and needs no answer.
        _ => {
            *slot = None;
            return Answer::Unrelated;
        }
    };
    match verdict(text) {
        Some(true) => {
            *slot = None;
            drop(slot);
            *GRANT.lock().await = Some((pending.command.clone(), std::time::Instant::now()));
            Answer::Approved(pending)
        }
        Some(false) => {
            *slot = None;
            Answer::Denied(pending)
        }
        // The user moved on. The question goes with them rather than lying in wait for a
        // "yes" typed an hour later about something else entirely.
        None => {
            *slot = None;
            Answer::Unrelated
        }
    }
}

/// What this turn left for the user: a question to answer, or a refusal to be told about.
pub async fn outcome() -> Option<Outcome> {
    OUTCOME.lock().await.clone()
}

/// Decide an approval for a turn the user is driving from their phone.
///
/// `Some` means the desktop prompt is not the right place to ask — the caller returns
/// this instead of opening a modal on a machine nobody is sitting at, where it blocks
/// the turn until it times out and the user has to walk to the PC to clear it.
///
/// Anything irreversible goes to the agent the team answers to first, with what it would
/// destroy in front of it. A refusal there ends the command; everything else still
/// reaches the user, now carrying the reviewer's line. See [`crate::ai::escalation`] for
/// why that narrowing is the whole value of it.
///
/// The grant is deliberately narrow: one command, matched byte for byte, spent on first
/// use and expiring on its own. "Yes" answers the command that was shown, not the next
/// one the agent thinks of.
pub async fn intercept_approval(
    db: &crate::storage::Db,
    session_id: &str,
    command: &str,
    shown: &str,
    irreversible: bool,
) -> Option<Result<(), String>> {
    let Some(ctx) = turn_context() else {
        return None;
    };
    if session_id != CONVERSATION_ID {
        return None;
    }

    let mut grant = GRANT.lock().await;
    match grant.clone() {
        Some((granted, at)) if at.elapsed() <= GRANT_TTL && granted == command => {
            *grant = None;
            return Some(Ok(()));
        }
        Some((_, at)) if at.elapsed() > GRANT_TTL => *grant = None,
        _ => {}
    }
    drop(grant);

    let reviewed = if irreversible {
        crate::ai::escalation::review(db, ctx.persona_id.as_deref(), shown, &ctx.ask).await
    } else {
        None
    };

    if let Some(v) = reviewed.as_ref().filter(|v| !v.ok) {
        *OUTCOME.lock().await = Some(Outcome::Vetoed {
            line: v.line(),
            shown: shown.to_string(),
        });
        return Some(Err(format!(
            "not run: {} — the agent you answer to reviewed this and refused it. Do not run \
             it, and do not look for another way to do the same thing. Say what you were \
             trying to achieve and stop; the user has been told.",
            v.line()
        )));
    }

    *OUTCOME.lock().await = Some(Outcome::Waiting(PendingApproval {
        command: command.to_string(),
        shown: shown.to_string(),
        reviewed: reviewed.map(|v| v.line()),
        asked_at: std::time::Instant::now(),
    }));

    Some(Err(
        "not run: this needs the user's approval, and they are on their phone rather than \
         at the desktop. The question has been sent to the chat for them to answer. Do not \
         ask again, and do not look for a way around it — do whatever else the job needs, \
         then say in one line what is waiting on their answer and stop."
            .to_string(),
    ))
}

/// The lines appended to a reply that left something for the user.
///
/// Deterministic rather than left to the agent to remember: the user has to be able to
/// see what they are approving, and a model that forgets to repeat it turns "yes" into
/// a guess.
pub fn approval_footer(o: &Outcome) -> String {
    let clip = |s: &str| {
        let s = s.trim();
        if s.chars().count() > 700 {
            s.chars().take(700).collect::<String>() + "…"
        } else {
            s.to_string()
        }
    };
    match o {
        Outcome::Waiting(p) => {
            let reviewed = match &p.reviewed {
                Some(line) => format!("{line}\n\n"),
                None => String::new(),
            };
            format!(
                "\n\n— waiting on you —\n{reviewed}{}\n\nReply *da* / *yes* to run it, \
                 *nu* / *no* to skip it.",
                clip(&p.shown)
            )
        }
        Outcome::Vetoed { line, shown } => format!(
            "\n\n— stopped —\n{line}\n\n{}",
            clip(shown)
        ),
    }
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

    /// Show the platform's "typing…" while a turn runs.
    ///
    /// A turn can take a minute, and until it answers there is nothing at all to say it
    /// heard you — on a phone that is indistinguishable from a bridge that is down. Best
    /// effort by design: a failed indicator must never stop the reply, and every
    /// platform expires it on its own, so a crash mid-turn leaves nothing stuck on.
    async fn set_typing(&mut self, _to: &IncomingMessage, _on: bool) -> Result<(), String> {
        Ok(())
    }

    /// Put an emoji on the user's message the moment it is picked up.
    ///
    /// The one signal that survives a locked phone: it lands on their message, stays
    /// there, and is readable from the chat list. Best effort like the typing indicator
    /// — a platform that refuses the emoji must never cost the reply.
    async fn react(&mut self, _to: &IncomingMessage, _emoji: &str) -> Result<(), String> {
        Ok(())
    }

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
        use tauri::{Emitter, Manager};
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
                crate::diag(&format!(
                    "remote({}): incoming message id={} chat={} from={} is_bot={}: {:?}",
                    kind.as_str(),
                    msg.id,
                    msg.chat_id,
                    msg.author.id,
                    msg.is_bot,
                    msg.content
                ));

                let ask = match authorize(&cfg, &msg) {
                    Ok(text) => text,
                    Err(rej) => {
                        let reason = match rej {
                            Rejected::Disabled => "transport is disabled or not configured",
                            Rejected::WrongChannel => "message in wrong channel",
                            Rejected::FromBot => "message from bot itself (echo)",
                            Rejected::NotAllowed => "sender is not in the allowlist",
                            Rejected::NoPrefix => "message missing required command prefix",
                            Rejected::Empty => "empty message content",
                        };
                        crate::diag(&format!(
                            "remote({}): refused message from {} ({}): {} [prefix={:?}, content={:?}]",
                            kind.as_str(),
                            msg.author.display_name,
                            msg.author.id,
                            reason,
                            cfg.prefix,
                            msg.content
                        ));
                        let _ = app.emit("remote://activity", serde_json::json!({
                            "kind": kind.as_str(),
                            "status": "rejected",
                            "reason": reason,
                            "sender": msg.author.id,
                            "name": msg.author.display_name,
                            "chat": msg.chat_id,
                            "content": msg.content,
                            "time": chrono::Local::now().format("%H:%M:%S").to_string(),
                        }));
                        continue;
                    }
                };

                crate::diag(&format!(
                    "remote({}): running a command from {} ({}): {:?}",
                    kind.as_str(),
                    msg.author.display_name,
                    msg.author.id,
                    ask
                ));

                // Before anything slow: a sent tick says the message left the phone, and
                // nothing at all says it was picked up. The emoji says which, and which
                // job it was read as. Failures are ignored — a missing reaction must never
                // cost the reply.
                let mark = reaction::for_message(kind, &ask);
                if let Err(e) = transport.react(&msg, mark).await {
                    crate::diag(&format!("remote({}): could not react: {e}", kind.as_str()));
                }

                // A command the last turn asked about, answered by this message. Resolved
                // here rather than inside the turn: the poll loop is what reads the chat,
                // so a turn blocked waiting for an answer is a turn that can never receive
                // one.
                let ask = match answer_pending(&ask).await {
                    Answer::Approved(p) => format!(
                        "The user has just approved this on their phone:\n\n{}\n\nRun it now — \
                         exactly the command you proposed, unchanged — and report what it did. \
                         Their message was: {ask}",
                        p.shown.trim()
                    ),
                    Answer::Denied(p) => format!(
                        "The user has just refused this:\n\n{}\n\nDo not run it, and do not \
                         look for another way to do the same thing. Their message was: {ask}",
                        p.shown.trim()
                    ),
                    Answer::Unrelated => ask,
                };

                let _ = app.emit("remote://activity", serde_json::json!({
                    "kind": kind.as_str(),
                    "status": "executing",
                    "reason": "running agent turn",
                    "sender": msg.author.id,
                    "name": msg.author.display_name,
                    "chat": msg.chat_id,
                    "content": ask,
                    "time": chrono::Local::now().format("%H:%M:%S").to_string(),
                }));

                // Recorded before the turn, not after: a turn that takes a minute should
                // not leave "where we last spoke" pointing at the previous platform.
                let here = Route { kind, chat_id: msg.chat_id.clone() };
                remember_route(&db, &here);

                // Shown before the lock, so somebody waiting behind another turn still
                // sees that their message landed. Failures are ignored on purpose: an
                // indicator is a courtesy, and none of them is worth losing a reply over.
                let _ = transport.set_typing(&msg, true).await;
                let (reply, redirect) = {
                    let _turn = TURN_LOCK.lock().await;
                    run_remote_turn(&app, &cfg, &ask).await
                };
                let _ = transport.set_typing(&msg, false).await;

                // A turn that stopped for an approval has to say so where the user is,
                // with the command in front of them and the two words that answer it.
                let reply = match outcome().await {
                    Some(o) => format!("{}{}", reply.trim_end(), approval_footer(&o)),
                    None => reply,
                };



                crate::diag(&format!(
                    "remote({}): turn finished, sending reply: {:?}",
                    kind.as_str(),
                    reply
                ));

                let _ = app.emit("remote://activity", serde_json::json!({
                    "kind": kind.as_str(),
                    "status": "replied",
                    "reason": "reply sent",
                    "sender": msg.author.id,
                    "name": msg.author.display_name,
                    "chat": msg.chat_id,
                    "content": reply,
                    "time": chrono::Local::now().format("%H:%M:%S").to_string(),
                }));

                // Answer where the message came from, unless the user asked to move.
                let moved = redirect
                    .filter(|k| *k != kind)
                    .and_then(|k| route_for(&db, k));
                match moved {
                    Some(route) => {
                        if let Err(e) = send_to(&route, &reply).await {
                            // The move failed, so the answer still has to reach somebody:
                            // fall back to the chat that asked for it rather than dropping
                            // a reply the user is waiting for.
                            crate::diag(&format!(
                                "remote({}): could not move the reply to {}: {e}",
                                kind.as_str(),
                                route.kind.as_str()
                            ));
                            let _ = transport.send(&cfg, &msg, &reply).await;
                        } else {
                            remember_route(&db, &route);
                        }
                    }
                    None => {
                        if let Err(e) = transport.send(&cfg, &msg, &reply).await {
                            crate::diag(&format!("remote({}): could not reply: {e}", kind.as_str()));
                        }
                    }
                }
            }
        }
    });
}

/// Run one agent turn on behalf of a remote message.
///
/// Returns what to send back, and the transport to send it to if the user asked to carry
/// on elsewhere.
async fn run_remote_turn(
    app: &tauri::AppHandle,
    cfg: &Config,
    ask: &str,
) -> (String, Option<Kind>) {
    use tauri::Manager;
    let db = app.state::<crate::storage::Db>().inner().clone();
    let hooks_cfg = if db.get_setting("agent.hooks_enabled").ok().flatten().as_deref()
        == Some("false")
    {
        crate::ai::hooks::HooksConfig::default()
    } else {
        crate::ai::hooks::HooksConfig::load(&app.state::<crate::ai::AgentHome>().inner().clone())
    };

    // Who is answering. A configured agent runs *as* itself — its standing
    // instructions, its servers, its trust level, its model — rather than the main
    // agent relaying to it, which is what put a middleman between the user and the
    // agent they set up to lead the team.
    let persona = cfg
        .persona_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|id| crate::ai::persona::resolve(&db, id));

    // Marks the turn as one the user is watching from a phone, for as long as it runs.
    // `safety::authorize` reads this to decide that the desktop is the wrong place to
    // ask, and who reviews anything that cannot be undone. Cleared however the turn ends.
    let _remote = RemoteTurnGuard::enter(persona.as_ref().map(|p| p.id.clone()), ask);

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
        // One id for the whole bridge, so a follow-up typed on another platform lands in
        // the same thread. Everyone on an allowlist therefore shares context — see the
        // module docs, which weigh that against what an allowlist already grants.
        session_id: CONVERSATION_ID.to_string(),
        // The agent's own servers when it has them, so a remote turn is not quietly
        // broader than the same agent doing the same work from the desktop.
        targets: crate::ai::persona::effective_targets(persona.as_ref(), &cfg.targets),
        // A persona's trust level wins. It is how the user said "this one may restart
        // services unattended", and a remote turn is exactly the unattended case.
        // Invalid values fall through to the remote setting, then to allowlist — never
        // to a silent "approve" that would stall every command on a phone.
        safety: persona
            .as_ref()
            .and_then(|p| p.safety_mode.clone())
            .filter(|m| matches!(m.as_str(), "full" | "allowlist" | "approve"))
            .or_else(|| {
                matches!(cfg.safety_mode.as_str(), "full" | "allowlist" | "approve")
                    .then(|| cfg.safety_mode.clone())
            })
            .unwrap_or_else(|| "allowlist".into()),
        plan_mode: false,
        workspace_id: None,
        canvas: Vec::new(),
        edits: crate::ai::edits::EditJournal::with_db(db.clone()),
        hooks: hooks_cfg,
        turn_images: Vec::new(),
        goal_id: None,
        persona_id: persona.as_ref().map(|p| p.id.clone()),
        read_only: false,
    };

    // Who it is, who reports to it, and what its colleagues have said to it since last
    // time. Delivered in the prompt rather than left to be fetched: a message nobody
    // thought to check for is a message that never arrived.
    let persona_block = match persona.as_ref() {
        Some(p) => {
            let all = db.list_personas().unwrap_or_default();
            let mut block = crate::ai::persona::prompt_block(p);
            block.push_str(&crate::ai::persona::hierarchy_block(&all, p));
            block.push_str(&crate::ai::persona::gitops_block(p));
            block.push_str(
                "\n\nYou are answering the user directly over chat. Do not hand this \
                 conversation to another agent just to have it answered — delegate only \
                 work that genuinely belongs to a colleague, and reply yourself either \
                 way.\n\n",
            );
            block
        }
        None => String::new(),
    };

    let prompt = format!(
        "{persona_block}This request arrived over remote chat, not from the desktop app. The user is \
         on their phone: keep the reply short and plain-text (no wide tables, no long \
         file dumps).\n\n\
         They cannot see a dialog on the desktop, so nothing may wait on one. Decide the \
         ordinary calls yourself, and where the answer belongs to a colleague — their \
         servers, their part of the system — ask that colleague rather than the user; that \
         is what the team is for. Ask the user only for what is genuinely theirs to decide, \
         and then by saying it in your reply and stopping, never with a tool that opens a \
         dialog. If a command needs their approval, the question reaches their chat on its \
         own: do the rest of the job, say in one line what is waiting, and stop.\n\n\
         You are on {arrived}, and earlier messages in this thread may have arrived on a \
         different app — it is one conversation either way. If the user asks to carry on \
         somewhere else, call remote_reply_on and answer normally; do not describe the \
         move, just make it.\n\n{ask}",
        arrived = cfg.kind.as_str(),
        persona_block = persona_block,
    );

    // The thread so far. Trimmed on save, so this is already bounded.
    let history = load_history(&db);
    let mut request = history.clone();
    request.push(crate::ai::provider::ChatMessage::user(prompt));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::ai::provider::StreamEvent>();
    // The reply is the final message, so nothing here needs the stream — except when
    // there is no final message. Draining it into the void meant a turn that produced
    // nothing could only be reported as "(the agent finished without saying anything)",
    // which says neither what went wrong nor where to look. The errors and the last few
    // status lines are exactly the missing explanation, so they are kept.
    let trace = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let trace_sink = trace.clone();
    let drain = tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let line = match ev {
                crate::ai::provider::StreamEvent::Error(e) => Some(format!("error: {e}")),
                crate::ai::provider::StreamEvent::Status(s) => Some(s),
                _ => None,
            };
            if let Some(line) = line {
                let mut t = trace_sink.lock().unwrap();
                // A cap, because a long tool-using turn emits hundreds of these and only
                // the end of it explains an empty answer.
                if t.len() >= 12 {
                    t.remove(0);
                }
                t.push(line);
            }
        }
    });

    // `conversation: false` like every other non-desktop caller — the flag gates
    // preference-learning and skill autopilot, not whether history is carried.
    // A persona can pin its own provider and model, and that has to hold here too, or
    // the same agent answers on a different model depending on where it was asked.
    let choice = match persona.as_ref() {
        Some(p) => crate::ai::registry::ModelChoice {
            provider_id: p.provider_id.clone(),
            model: p.model.clone(),
        },
        None => crate::ai::registry::ModelChoice::active(),
    };
    let result = crate::ai::agent::run_turn(&tc, choice, request, false, &tx).await;
    drop(tx);
    let _ = drain.await;

    let redirect = tc
        .session_state
        .reply_route(&tc.session_id)
        .as_deref()
        .and_then(Kind::parse);

    let reply = match result {
        Ok(msg) if !msg.content.trim().is_empty() => {
            // What the user actually typed goes into the thread, not the decorated
            // request — otherwise every past turn carries another copy of the "you are
            // talking to someone on their phone" preamble into the next one's context.
            //
            // Only a turn that produced an answer joins the thread at all. Recording a
            // failure would teach the next turn that the last thing said was an error.
            let mut thread = history;
            thread.push(crate::ai::provider::ChatMessage::user(ask.to_string()));
            thread.push(msg.clone());
            save_history(&db, &thread);
            msg.content
        }
        Ok(_) => {
            // Empty is not an answer, and "it said nothing" is not a diagnosis. Whatever
            // the run did emit is the only account of why, so it goes to the log in full
            // and to the phone in short.
            let trace = trace.lock().unwrap().clone();
            crate::diag(&format!(
                "remote: turn produced no text. Trace:\n{}",
                if trace.is_empty() { "(nothing was emitted either)".into() } else { trace.join("\n") }
            ));
            match trace.iter().rev().find(|l| l.starts_with("error: ")) {
                Some(e) => format!("The agent could not answer. {}", e.trim_start_matches("error: ")),
                None => format!(
                    "The agent ran but produced no answer{}. Check Settings > Remote \
                     control for the full trace.",
                    trace.last().map(|l| format!(" (last step: {l})")).unwrap_or_default()
                ),
            }
        }
        Err(e) => format!("Failed: {e}"),
    };
    (reply, redirect)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_yes_is_only_a_yes_when_the_whole_message_is_one() {
        assert_eq!(verdict("da"), Some(true));
        assert_eq!(verdict("DA!"), Some(true));
        assert_eq!(verdict("ok hai"), Some(true));
        assert_eq!(verdict("yes run it"), Some(true));
        assert_eq!(verdict("👍"), Some(true));
        assert_eq!(verdict("nu"), Some(false));
        assert_eq!(verdict("no, stop"), Some(false));

        // The message that matters most: it opens with "nu" and is not a refusal, it is
        // the next job. Reading it as an answer would throw it away and leave the user
        // waiting on an agent that thinks it was told to stop.
        assert_eq!(verdict("nu merge site-ul"), None);
        assert_eq!(verdict("da-mi logurile de la nginx"), None);
        assert_eq!(verdict(""), None);
        // Ambiguity is never a yes.
        assert_eq!(verdict("da nu"), Some(false));
    }

    #[tokio::test]
    async fn a_command_is_approved_from_the_phone_and_the_yes_is_spent() {
        let _turn = RemoteTurnGuard::enter(None, "restarteaza nginx");
        let db = db();
        // Nothing irreversible in this test, so no reviewer is consulted and no model is
        // reached: `false` is the whole of that decision.
        let ask = |command: &'static str, shown: &'static str| {
            let db = &db;
            async move { intercept_approval(db, CONVERSATION_ID, command, shown, false).await }
        };

        // A desktop session is untouched: it still gets the desktop prompt.
        assert!(
            intercept_approval(&db, "desktop-session", "systemctl restart nginx", "…", false)
                .await
                .is_none()
        );

        // A turn driven from a chat asks there instead, and stops rather than blocking
        // on a modal on a machine nobody is sitting at.
        let asked = ask("systemctl restart nginx", "systemctl restart nginx").await;
        assert!(matches!(asked, Some(Err(_))), "the turn should be told to stop and ask");
        let waiting = outcome().await.expect("the question is waiting for an answer");
        let footer = approval_footer(&waiting);
        assert!(footer.contains("systemctl restart nginx"), "{footer}");
        assert!(footer.contains("da"), "the user has to be told how to answer: {footer}");

        // "da" runs it — once.
        assert!(matches!(answer_pending("da").await, Answer::Approved(_)));
        assert!(outcome().await.is_none(), "an answered question stops waiting");
        assert!(ask("systemctl restart nginx", "…")
            .await
            .expect("still the remote path")
            .is_ok());
        assert!(
            matches!(ask("systemctl restart nginx", "…").await, Some(Err(_))),
            "a spent yes must not authorise the same command twice"
        );

        // And a yes covers the command it was shown, not the next one the agent thinks of.
        assert!(matches!(answer_pending("ok").await, Answer::Approved(_)));
        assert!(
            matches!(ask("rm -rf /srv/app", "…").await, Some(Err(_))),
            "the grant is for one exact command"
        );

        // A message about something else takes the question with it, so a "yes" typed an
        // hour later cannot land on a command nobody remembers proposing.
        assert!(matches!(answer_pending("cate servere sunt sus?").await, Answer::Unrelated));
        assert!(outcome().await.is_none());

        // A refusal, in the same test because the question is one per conversation and
        // lives in a static: two tests holding it at once would only be testing each
        // other.
        assert!(matches!(
            ask("rm -rf /srv/data", "rm -rf /srv/data").await,
            Some(Err(_))
        ));
        assert!(matches!(answer_pending("nu").await, Answer::Denied(_)));
        assert!(outcome().await.is_none());
        // The refusal is not a grant: asking again asks again.
        assert!(matches!(ask("rm -rf /srv/data", "…").await, Some(Err(_))));
        let _ = answer_pending("nu").await;
    }

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
            persona_id: None,
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

    fn db() -> crate::storage::Db {
        crate::storage::Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn a_route_survives_the_round_trip_through_settings() {
        for chat in ["chan-1", "40712345678@s.whatsapp.net", "a:b:c"] {
            let r = Route { kind: Kind::WhatsApp, chat_id: chat.into() };
            assert_eq!(Route::decode(&r.encode()), Some(r), "{chat}");
        }
    }

    #[test]
    fn a_route_with_no_chat_or_no_transport_is_not_a_route() {
        // Empty settings read back as an empty string, so "telegram:" must not decode
        // into a route that then sends a message to nowhere.
        assert_eq!(Route::decode("telegram:"), None);
        assert_eq!(Route::decode("carrier-pigeon:x"), None);
        assert_eq!(Route::decode(""), None);
    }

    #[test]
    fn moving_a_conversation_prefers_where_we_last_spoke() {
        let d = db();
        // Restricted to one channel, but the last exchange was somewhere else — that is
        // where the user is actually reading, so that is where a move should land.
        d.set_setting(&Kind::Discord.setting_chat(), "configured-chan").unwrap();
        assert_eq!(
            route_for(&d, Kind::Discord).map(|r| r.chat_id),
            Some("configured-chan".into())
        );

        remember_route(&d, &Route { kind: Kind::Discord, chat_id: "live-chan".into() });
        assert_eq!(
            route_for(&d, Kind::Discord).map(|r| r.chat_id),
            Some("live-chan".into())
        );
    }

    #[test]
    fn a_transport_never_used_and_never_configured_has_nowhere_to_go() {
        // The honest answer is "I don't know where to write", not a guess. `remote_notify`
        // and `remote_reply_on` both refuse on this.
        assert!(route_for(&db(), Kind::Telegram).is_none());
    }

    #[test]
    fn the_last_route_is_global_but_each_transport_keeps_its_own() {
        let d = db();
        remember_route(&d, &Route { kind: Kind::Telegram, chat_id: "tg-1".into() });
        remember_route(&d, &Route { kind: Kind::WhatsApp, chat_id: "wa-1".into() });

        // Most recent wins for "answer where we last spoke"...
        assert_eq!(last_route(&d), Some(Route { kind: Kind::WhatsApp, chat_id: "wa-1".into() }));
        // ...but Telegram is still reachable by name, which is what "go back to Telegram"
        // needs.
        assert_eq!(route_for(&d, Kind::Telegram).map(|r| r.chat_id), Some("tg-1".into()));
    }

    #[test]
    fn the_shared_thread_keeps_its_tail_and_survives_a_reload() {
        let d = db();
        let long: Vec<crate::ai::provider::ChatMessage> = (0..HISTORY_LIMIT + 10)
            .map(|i| crate::ai::provider::ChatMessage::user(format!("m{i}")))
            .collect();
        save_history(&d, &long);

        let back = load_history(&d);
        assert_eq!(back.len(), HISTORY_LIMIT, "history is bounded");
        // Trimmed from the front: a follow-up refers to the most recent turns, so those
        // are the ones that must survive.
        assert_eq!(back.last().unwrap().content, format!("m{}", HISTORY_LIMIT + 9));
    }

    #[test]
    fn an_empty_thread_reads_as_empty_rather_than_failing() {
        assert!(load_history(&db()).is_empty());
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
    fn picking_a_chat_shuts_out_every_other_one() {
        // The reported problem: with no chat set, the bridge reads every conversation
        // the linked WhatsApp account takes part in — including ones with other people
        // — and weighs each against the allowlist. Naming a chat has to actually
        // confine it, and the ids involved are full JIDs the user picks from a list.
        let mut c = cfg();
        c.kind = Kind::WhatsApp;
        c.chat_id = "120363000000000000@g.us".into();

        let mut here = msg();
        here.chat_id = "120363000000000000@g.us".into();
        assert_eq!(authorize(&c, &here), Ok("restart nginx".into()));

        // Same allowed person, different conversation. Being allowed to command the
        // agent must not mean every chat they are in is read.
        let mut elsewhere = msg();
        elsewhere.chat_id = "120363999999999999@g.us".into();
        assert_eq!(authorize(&c, &elsewhere), Err(Rejected::WrongChannel));

        let mut dm = msg();
        dm.chat_id = "40799999999@s.whatsapp.net".into();
        assert_eq!(authorize(&c, &dm), Err(Rejected::WrongChannel));
    }

    #[test]
    fn a_note_to_self_chat_is_matched_however_it_is_written() {
        // The chat people most want to restrict to is their own. Its id is their own
        // number, which they may have typed by hand before the picker existed, so the
        // bare number and the full JID have to mean the same chat.
        let mut c = cfg();
        c.kind = Kind::WhatsApp;
        let mut m = msg();
        m.chat_id = "393805978429@s.whatsapp.net".into();

        for stored in ["393805978429@s.whatsapp.net", "393805978429"] {
            c.chat_id = stored.into();
            assert_eq!(authorize(&c, &m), Ok("restart nginx".into()), "stored as {stored}");
        }
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
    fn a_reply_says_it_came_from_the_agent() {
        // On WhatsApp the bridge replies from the user's own account, so its messages
        // sit in the same column and the same colour as everything they typed. Without
        // a marker there is nothing at all to tell them apart when scrolling back.
        assert_eq!(mark_as_agent("all good"), "[xConsole] all good");
        assert!(mark_as_agent("done").starts_with(AGENT_PREFIX));
    }

    #[test]
    fn marking_twice_does_not_stack() {
        // A reply can be sent more than once — moved to another app, retried after a
        // failed send — and two prefixes read as a bug.
        let once = mark_as_agent("uptime is 16 days");
        assert_eq!(mark_as_agent(&once), once);
        assert_eq!(once.matches(AGENT_PREFIX).count(), 1);
    }

    #[test]
    fn every_part_of_a_long_answer_is_marked_not_just_the_first() {
        // A long answer arrives as several messages, and on WhatsApp they all come from
        // the user's own account. An unmarked second message is indistinguishable from
        // something they typed themselves.
        let chunks = agent_chunks(Kind::Discord, &(0..400).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"));
        assert!(chunks.len() > 1, "needs to actually split");
        for c in &chunks {
            assert!(c.starts_with(AGENT_PREFIX), "unmarked part: {c}");
        }
    }

    #[test]
    fn marking_never_pushes_a_chunk_over_the_platform_limit() {
        // Discord rejects an over-long message outright rather than truncating it, so a
        // chunk that fit before the prefix and not after is a message that never
        // arrives. The prefix comes out of the budget, not on top of it.
        for kind in Kind::ALL {
            let chunks = agent_chunks(kind, &"y".repeat(9000));
            assert!(
                chunks.iter().all(|c| c.len() <= kind.max_chars()),
                "{} produced an over-long chunk",
                kind.as_str()
            );
            // Nothing is dropped in the process.
            let body: String = chunks
                .iter()
                .map(|c| c.trim_start_matches(AGENT_PREFIX).trim_start())
                .collect();
            assert_eq!(body.len(), 9000, "{} lost content", kind.as_str());
        }
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
