//! Discord transport: poll one channel's messages over the REST API.
//!
//! Onboarding is the worst of the three — a developer-portal application, a bot user,
//! a token, an invite with the right intents — which is exactly why Telegram and
//! WhatsApp exist alongside it rather than instead of it. Nothing here reaches inward:
//! it is an outbound GET on a timer.

use super::{Author, Config, IncomingMessage, Kind, Transport};

/// Discord's REST base. Pinned to a version so a future default cannot change the
/// response shape underneath us.
const DISCORD_API: &str = "https://discord.com/api/v10";

#[derive(Default)]
pub struct Discord {
    /// Newest message id already seen. `None` replays nothing: the first poll after
    /// arming reads only what arrives from then on.
    after: Option<String>,
}

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
            let author = v.get("author")?;
            Some(IncomingMessage {
                id: v.get("id")?.as_str()?.to_string(),
                chat_id: v.get("channel_id").and_then(|c| c.as_str()).unwrap_or(channel_id).to_string(),
                author: Author {
                    id: author.get("id")?.as_str()?.to_string(),
                    username: author
                        .get("username")
                        .and_then(|u| u.as_str())
                        .map(str::to_string),
                    display_name: author
                        .get("global_name")
                        .and_then(|u| u.as_str())
                        .or_else(|| author.get("username").and_then(|u| u.as_str()))
                        .unwrap_or("someone")
                        .to_string(),
                },
                is_bot: author.get("bot").and_then(|b| b.as_bool()).unwrap_or(false),
                content: v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string(),
            })
        })
        .collect())
}

/// Post a reply, split into platform-sized messages.
pub async fn send_message(token: &str, channel_id: &str, text: &str) -> Result<(), String> {
    for chunk in super::agent_chunks(Kind::Discord, text) {
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

#[async_trait::async_trait]
impl Transport for Discord {
    fn kind(&self) -> Kind {
        Kind::Discord
    }

    async fn poll(&mut self, cfg: &Config) -> Result<Vec<IncomingMessage>, String> {
        let Some(token) = super::load_token(Kind::Discord) else {
            return Ok(vec![]);
        };
        let msgs = fetch_messages(&token, &cfg.chat_id, self.after.as_deref()).await?;
        // Advance past every message fetched, authorised or not — otherwise one
        // un-actionable message is re-fetched forever.
        if let Some(last) = msgs.last() {
            self.after = Some(last.id.clone());
        }
        Ok(msgs)
    }

    async fn send(&mut self, _cfg: &Config, to: &IncomingMessage, text: &str) -> Result<(), String> {
        let token = super::load_token(Kind::Discord).ok_or("no discord token saved")?;
        send_message(&token, &to.chat_id, text).await
    }

    /// Discord's typing endpoint. Lasts about ten seconds and cannot be cancelled, so
    /// `on == false` has nothing to do — and a turn that dies leaves nothing stuck.
    async fn set_typing(&mut self, to: &IncomingMessage, on: bool) -> Result<(), String> {
        if !on {
            return Ok(());
        }
        let token = super::load_token(Kind::Discord).ok_or("no discord token saved")?;
        let resp = client()
            .post(format!("{DISCORD_API}/channels/{}/typing", to.chat_id))
            .header("Authorization", format!("Bot {token}"))
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        resp.status()
            .is_success()
            .then_some(())
            .ok_or_else(|| format!("discord refused the typing indicator: {}", resp.status()))
    }

    fn reset(&mut self) {
        self.after = None;
    }
}
