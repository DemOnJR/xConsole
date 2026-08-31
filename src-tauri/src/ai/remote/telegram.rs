//! Telegram transport: `getUpdates` long-poll over the Bot API.
//!
//! The cheapest bridge to set up of the three — one conversation with @BotFather
//! produces a token, and the user is done. Like the others it only ever makes outbound
//! HTTPS requests, so nothing about it opens this machine up.

use super::{Author, Config, IncomingMessage, Kind, Transport};

#[derive(Default)]
pub struct Telegram {
    /// `update_id` to ask for next. `None` means the backlog has not been skipped yet.
    offset: Option<i64>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

fn api(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}

/// Unwrap Telegram's `{"ok":true,"result":…}` envelope.
///
/// The error branch deliberately reports only `description`, never the whole body: a
/// failed request echoes the URL, and the URL contains the bot token.
fn unwrap_result(v: serde_json::Value) -> Result<serde_json::Value, String> {
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
    }
    Err(format!(
        "telegram refused: {}",
        v.get("description").and_then(|d| d.as_str()).unwrap_or("unknown error")
    ))
}

async fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = client().get(url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        // Prefer Telegram's own description; fall back to the code alone, never the
        // request URL.
        return Err(unwrap_result(body).err().unwrap_or_else(|| format!("telegram returned {status}")));
    }
    unwrap_result(body)
}

/// Ask Telegram who this token belongs to. Used by the settings screen to turn "is my
/// token right?" into a stated answer instead of a silent bridge.
pub async fn get_me(token: &str) -> Result<String, String> {
    let me = get_json(&api(token, "getMe")).await?;
    Ok(me
        .get("username")
        .and_then(|u| u.as_str())
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| "the bot".to_string()))
}

/// Turn one `getUpdates` page into normalised messages, and work out the next offset.
///
/// Pure, and shared with [`fetch_updates`] rather than reimplemented for tests — the
/// parsing is where the surprises live (negative group ids, updates carrying no text),
/// so a test that exercised a copy of it would prove nothing about what runs.
fn parse_page(
    result: &serde_json::Value,
    offset: Option<i64>,
) -> (Vec<IncomingMessage>, Option<i64>) {
    let mut next = offset;
    let mut out = Vec::new();
    for u in result.as_array().map(Vec::as_slice).unwrap_or_default() {
        // Advance past every update, including ones with no text — a sticker in the
        // ops channel must not be re-fetched forever, blocking everything behind it.
        if let Some(id) = u.get("update_id").and_then(|v| v.as_i64()) {
            next = Some(next.map_or(id + 1, |n| n.max(id + 1)));
        }
        let Some(m) = u.get("message") else { continue };
        let Some(from) = m.get("from") else { continue };
        let Some(chat_id) = m.get("chat").and_then(|c| c.get("id")) else { continue };
        let Some(author_id) = from.get("id") else { continue };
        let display_name = match (
            from.get("first_name").and_then(|v| v.as_str()),
            from.get("last_name").and_then(|v| v.as_str()),
        ) {
            (Some(f), Some(l)) => format!("{f} {l}"),
            (Some(f), None) => f.to_string(),
            _ => from
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("someone")
                .to_string(),
        };
        out.push(IncomingMessage {
            id: m.get("message_id").map(|v| v.to_string()).unwrap_or_default(),
            // Ids arrive as JSON numbers, and group ids are negative. `to_string` on
            // the `Value` would keep the quotes for a string-typed id, which then fails
            // to match the configured chat and silently ignores every message.
            chat_id: chat_id.to_string().trim_matches('"').to_string(),
            author: Author {
                id: author_id.to_string().trim_matches('"').to_string(),
                username: from.get("username").and_then(|v| v.as_str()).map(str::to_string),
                display_name,
            },
            is_bot: from.get("is_bot").and_then(|v| v.as_bool()).unwrap_or(false),
            content: m.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        });
    }
    (out, next)
}

/// Fetch one `getUpdates` page. Returns the next offset alongside the messages, so the
/// cursor advances past updates that carry nothing we act on.
pub async fn fetch_updates(
    token: &str,
    offset: Option<i64>,
) -> Result<(Vec<IncomingMessage>, Option<i64>), String> {
    let mut url = format!(
        "{}?limit=20&timeout=0&allowed_updates=%5B%22message%22%5D",
        api(token, "getUpdates")
    );
    if let Some(offset) = offset {
        url.push_str(&format!("&offset={offset}"));
    }
    let result = get_json(&url).await?;
    Ok(parse_page(&result, offset))
}

/// Post a reply, split into platform-sized messages.
pub async fn send_message(token: &str, chat_id: &str, text: &str) -> Result<(), String> {
    for chunk in super::agent_chunks(Kind::Telegram, text) {
        let resp = client()
            .post(api(token, "sendMessage"))
            // Deliberately not `parse_mode`: command output is full of underscores and
            // asterisks, and Telegram rejects the whole message on a malformed entity.
            // A reply that arrives as plain text beats one that does not arrive.
            .json(&serde_json::json!({ "chat_id": chat_id, "text": chunk }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            return Err(unwrap_result(body)
                .err()
                .unwrap_or_else(|| format!("telegram rejected a reply: {status}")));
        }
    }
    Ok(())
}

/// Put one emoji on a message.
///
/// Telegram takes a *list*, and replaces whatever the bot had there before — so this
/// also moves the mark rather than stacking marks. Only emoji from its published set are
/// accepted; anything else fails the whole request, which is why the choice is made in
/// [`super::reaction`] per platform.
pub async fn set_reaction(
    token: &str,
    chat_id: &str,
    message_id: &str,
    emoji: &str,
) -> Result<(), String> {
    let id: i64 = message_id
        .trim()
        .parse()
        .map_err(|_| format!("not a telegram message id: {message_id:?}"))?;
    let resp = client()
        .post(api(token, "setMessageReaction"))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "message_id": id,
            "reaction": [{ "type": "emoji", "emoji": emoji }],
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        return Err(unwrap_result(body)
            .err()
            .unwrap_or_else(|| format!("telegram refused a reaction: {status}")));
    }
    Ok(())
}

#[async_trait::async_trait]
impl Transport for Telegram {
    fn kind(&self) -> Kind {
        Kind::Telegram
    }

    async fn poll(&mut self, _cfg: &Config) -> Result<Vec<IncomingMessage>, String> {
        let Some(token) = super::load_token(Kind::Telegram) else {
            return Ok(vec![]);
        };

        // Telegram holds undelivered updates for 24 hours, so a first poll with no
        // offset would replay everything sent while the bridge was off — commands
        // nobody was listening for, arriving as if they had just been given. Prime the
        // cursor from the newest update and act on nothing.
        if self.offset.is_none() {
            let (_, next) = fetch_updates(&token, Some(-1)).await?;
            self.offset = Some(next.unwrap_or(0));
            return Ok(vec![]);
        }

        let (msgs, next) = fetch_updates(&token, self.offset).await?;
        if next.is_some() {
            self.offset = next;
        }
        Ok(msgs)
    }

    async fn react(&mut self, to: &IncomingMessage, emoji: &str) -> Result<(), String> {
        let token = super::load_token(Kind::Telegram).ok_or("no telegram token saved")?;
        set_reaction(&token, &to.chat_id, &to.id, emoji).await
    }

    async fn send(&mut self, _cfg: &Config, to: &IncomingMessage, text: &str) -> Result<(), String> {
        let token = super::load_token(Kind::Telegram).ok_or("no telegram token saved")?;
        send_message(&token, &to.chat_id, text).await
    }

    /// Telegram's `sendChatAction`. It expires after about five seconds by itself, so
    /// there is nothing to clear and nothing left stuck if the turn dies mid-flight —
    /// hence no work at all for `on == false`.
    async fn set_typing(&mut self, to: &IncomingMessage, on: bool) -> Result<(), String> {
        if !on {
            return Ok(());
        }
        let token = super::load_token(Kind::Telegram).ok_or("no telegram token saved")?;
        let resp = client()
            .post(api(&token, "sendChatAction"))
            .json(&serde_json::json!({ "chat_id": to.chat_id, "action": "typing" }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        resp.status()
            .is_success()
            .then_some(())
            .ok_or_else(|| format!("telegram refused the typing indicator: {}", resp.status()))
    }

    fn reset(&mut self) {
        self.offset = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_body_never_leaks_the_token_bearing_url() {
        // Telegram puts the bot token in the path, so echoing a raw failure body into
        // the diagnostics log would write the credential to disk.
        let err = unwrap_result(serde_json::json!({
            "ok": false,
            "error_code": 401,
            "description": "Unauthorized"
        }))
        .unwrap_err();
        assert_eq!(err, "telegram refused: Unauthorized");
        assert!(!err.contains("api.telegram.org"));
    }

    #[test]
    fn the_cursor_advances_past_updates_we_ignore() {
        // A sticker in the ops channel carries no text. If it did not move the offset,
        // it would be re-fetched on every poll and block everything behind it.
        let result = serde_json::json!([
            {"update_id": 10, "message": {"message_id": 1, "chat": {"id": 5},
             "from": {"id": 7, "is_bot": false, "first_name": "Ada"}, "sticker": {}}},
            {"update_id": 11, "message": {"message_id": 2, "chat": {"id": 5},
             "from": {"id": 7, "is_bot": false, "first_name": "Ada"}, "text": "hello"}}
        ]);
        let (msgs, next) = parse_page(&result, None);
        assert_eq!(next, Some(12));
        // The sticker still normalises to a message with empty content; `authorize`
        // rejects it as Empty rather than this layer guessing.
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "hello");
        assert_eq!(msgs[1].chat_id, "5");
        assert_eq!(msgs[1].author.id, "7");
        assert_eq!(msgs[1].author.display_name, "Ada");
    }

    #[test]
    fn ids_are_plain_digits_not_json_numbers() {
        // Chat ids come back as JSON numbers and are negative for groups. Serialising
        // them naively would produce `"-1001234"` with quotes, which then fails to
        // match the configured chat id and silently ignores every message.
        let result = serde_json::json!([
            {"update_id": 1, "message": {"message_id": 9, "chat": {"id": -1001234567890i64},
             "from": {"id": 42, "is_bot": false, "first_name": "A", "last_name": "B",
                      "username": "ada"}, "text": "ping"}}
        ]);
        let (msgs, _) = parse_page(&result, None);
        assert_eq!(msgs[0].chat_id, "-1001234567890");
        assert_eq!(msgs[0].author.id, "42");
        assert_eq!(msgs[0].author.username.as_deref(), Some("ada"));
        assert_eq!(msgs[0].author.display_name, "A B");
    }

    #[test]
    fn a_bot_author_is_carried_through_so_authorize_can_refuse_it() {
        let result = serde_json::json!([
            {"update_id": 1, "message": {"message_id": 9, "chat": {"id": 5},
             "from": {"id": 42, "is_bot": true, "first_name": "Other"}, "text": "!x hi"}}
        ]);
        let (msgs, _) = parse_page(&result, None);
        assert!(msgs[0].is_bot);
    }

}
