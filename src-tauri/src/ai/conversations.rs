//! Persisted agent conversations with Hermes-style compact thread summaries.

use crate::ai::provider::ChatMessage;

/// Max chars injected into the system prompt for this thread's context.
pub const SUMMARY_PROMPT_MAX: usize = 900;

/// Auto title from the first user message.
pub fn derive_title(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .find(|m| m.role == "user" && !m.content.trim().is_empty())
        .map(|m| one_line(&m.content, 56))
        .unwrap_or_else(|| "New chat".into())
}

/// Build a terse bullet summary of recent turns (Hermes-style thread memory).
pub fn compact_summary(messages: &[ChatMessage]) -> String {
    if messages.is_empty() {
        return String::new();
    }

    let mut bullets: Vec<String> = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role == "user" && !messages[i].content.trim().is_empty() {
            bullets.push(format!("- {}", one_line(&messages[i].content, 110)));
            if i + 1 < messages.len()
                && messages[i + 1].role == "assistant"
                && !messages[i + 1].content.trim().is_empty()
            {
                bullets.push(format!("  → {}", one_line(&messages[i + 1].content, 110)));
                i += 1;
            }
        }
        i += 1;
    }

    // Keep only the most recent exchanges (newest at bottom).
    if bullets.len() > 8 {
        bullets = bullets.split_off(bullets.len() - 8);
    }

    let body = bullets.join("\n");
    truncate_block(&body, SUMMARY_PROMPT_MAX)
}

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    truncate_block(&flat, max)
}

fn truncate_block(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.trim().to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n…", s[..cut].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_stays_compact() {
        let msgs = vec![
            ChatMessage::user("Set up daily reboot on both VPS"),
            ChatMessage::assistant("Configured systemd timers on both servers."),
        ];
        let s = compact_summary(&msgs);
        assert!(s.contains("daily reboot"));
        assert!(s.len() <= SUMMARY_PROMPT_MAX + 4);
    }
}
