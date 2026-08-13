//! Interactive agent prompts that block the turn until the user responds, plus
//! per-session flags that those responses set.
//!
//! This generalizes the request→response pattern that [`crate::ai::safety`] uses
//! for command approvals (register a one-shot, emit an event to the UI, await the
//! decision). Approvals return a bool; questions (`ask_user`) and plan reviews
//! (`present_plan`) return free-form text, so they use [`PromptRegistry`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::oneshot;

use crate::ai::prefix_telemetry::RequestFingerprint;
use crate::ai::provider::ChatMessage;

/// Tracks in-flight `ask_user` / `present_plan` prompts so the UI can resolve
/// them with the user's answer. Managed Tauri state.
#[derive(Clone, Default)]
pub struct PromptRegistry {
    pending: Arc<DashMap<String, oneshot::Sender<String>>>,
    /// session_id → latest prompt id, so a new plan presentation can supersede
    /// the previous still-pending one.
    by_session: Arc<DashMap<String, String>>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a prompt id and get the receiver to await the user's answer.
    pub fn register(&self, id: String) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);
        rx
    }

    /// Register a prompt that belongs to a session (plans), so later prompts for
    /// the same session can supersede it.
    pub fn register_for_session(&self, id: String, session_id: &str) -> oneshot::Receiver<String> {
        let rx = self.register(id.clone());
        self.by_session.insert(session_id.to_string(), id);
        rx
    }

    /// Resolve (with "CANCEL: superseded") any pending prompt for this session
    /// other than `keep_id`, so a newer plan presentation replaces the old one.
    pub fn cancel_superseded(&self, session_id: &str, keep_id: &str) {
        if let Some((_, old_id)) = self.by_session.remove(session_id) {
            if old_id != keep_id {
                if let Some((_, tx)) = self.pending.remove(&old_id) {
                    let _ = tx.send("CANCEL: superseded".into());
                }
            }
        }
    }

    /// Deliver the user's answer to a waiting prompt. Returns true if it was awaiting.
    pub fn resolve(&self, id: &str, answer: String) -> bool {
        if let Some((_, tx)) = self.pending.remove(id) {
            // Drop the session mapping so it can't leak / supersede later.
            let session = self
                .by_session
                .iter()
                .find(|e| e.value() == id)
                .map(|e| e.key().clone());
            if let Some(s) = session {
                let _ = self.by_session.remove(&s);
            }
            let _ = tx.send(answer);
            true
        } else {
            false
        }
    }

    /// Drop a pending prompt without answering (e.g. on timeout).
    pub fn cancel(&self, id: &str) -> bool {
        if let Some((_, _)) = self.pending.remove(id) {
            let session = self
                .by_session
                .iter()
                .find(|e| e.value() == id)
                .map(|e| e.key().clone());
            if let Some(s) = session {
                let _ = self.by_session.remove(&s);
            }
            true
        } else {
            false
        }
    }

    /// Resolve every pending prompt for a session with the given answer (used
    /// by Stop so a blocked plan/question wait can be interrupted).
    pub fn resolve_all_for_session(&self, session_id: &str, answer: String) -> usize {
        let ids: Vec<String> = self
            .by_session
            .iter()
            .filter(|e| e.key() == session_id)
            .map(|e| e.value().clone())
            .collect();
        let mut n = 0;
        for id in ids {
            if self.resolve(&id, answer.clone()) {
                n += 1;
            }
        }
        n
    }
}

/// Per-conversation flags set by the user's interactive choices: a safety-mode
/// override ("don't ask again this chat" → full auto) and whether a plan has been
/// approved (lifts the plan-mode mutation guard). Managed Tauri state.
#[derive(Clone, Default)]
pub struct SessionState {
    map: Arc<DashMap<String, SessionFlags>>,
}

#[derive(Clone, Default)]
struct SessionFlags {
    /// Effective safety mode override for this session, if the user chose
    /// "don't ask again". Wins over the per-VPS and global settings.
    safety_override: Option<String>,
    /// Set once the user approves a plan, so subsequent mutating tools run.
    plan_approved: bool,
    /// Set when the user presses Stop. An `Arc<AtomicBool>` (not a plain bool) so a
    /// clone can be handed to the provider's streaming loop, letting Stop interrupt
    /// an in-flight model response immediately — not just between tool steps.
    cancelled: Arc<AtomicBool>,
    /// Last provider-visible message list (includes frozen `# Runtime context`
    /// blocks). Replayed on the next user turn so the prefix stays append-only.
    last_request_messages: Option<Vec<ChatMessage>>,
    /// Fingerprint of that last request, so classification is not `first_request`
    /// on every new `run_turn`.
    last_prefix: Option<RequestFingerprint>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Switch this session to full autonomy (no more approval prompts this chat).
    pub fn set_full_auto(&self, session_id: &str) {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .safety_override = Some("full".to_string());
    }

    /// The session's safety override, if any.
    pub fn safety_override(&self, session_id: &str) -> Option<String> {
        self.map.get(session_id).and_then(|f| f.safety_override.clone())
    }

    /// Mark that the user approved a plan for this session.
    pub fn mark_plan_approved(&self, session_id: &str) {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .plan_approved = true;
    }

    /// Whether a plan has been approved for this session.
    pub fn plan_approved(&self, session_id: &str) -> bool {
        self.map.get(session_id).map(|f| f.plan_approved).unwrap_or(false)
    }

    /// Clear plan approval at the beginning of a new agent turn.
    pub fn clear_plan_approved(&self, session_id: &str) {
        self.map.entry(session_id.to_string()).or_default().plan_approved = false;
    }

    /// Request the running turn to stop (user pressed Stop).
    pub fn cancel(&self, session_id: &str) {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .cancelled
            .store(true, Ordering::SeqCst);
    }

    /// Whether a stop has been requested for this session.
    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.map
            .get(session_id)
            .map(|f| f.cancelled.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// Clear the cancel flag (called at the start of each turn).
    pub fn clear_cancel(&self, session_id: &str) {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .cancelled
            .store(false, Ordering::SeqCst);
    }

    /// A shared handle to this session's cancel flag, for the provider's streaming
    /// loop to poll so Stop can interrupt a response mid-flight.
    pub fn cancel_flag(&self, session_id: &str) -> Arc<AtomicBool> {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .cancelled
            .clone()
    }

    /// Last messages actually sent to the provider for this session.
    pub fn last_request_messages(&self, session_id: &str) -> Option<Vec<ChatMessage>> {
        self.map
            .get(session_id)
            .and_then(|f| f.last_request_messages.clone())
    }

    pub fn store_request_messages(&self, session_id: &str, messages: Vec<ChatMessage>) {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .last_request_messages = Some(messages);
    }

    pub fn last_prefix(&self, session_id: &str) -> Option<RequestFingerprint> {
        self.map.get(session_id).and_then(|f| f.last_prefix.clone())
    }

    pub fn store_prefix(&self, session_id: &str, prefix: RequestFingerprint) {
        self.map
            .entry(session_id.to_string())
            .or_default()
            .last_prefix = Some(prefix);
    }

    /// Replay last provider request from disk after an app restart.
    pub fn load_prefix_cache(&self, data_dir: &std::path::Path, session_id: &str) {
        if self.last_request_messages(session_id).is_some() {
            return;
        }
        let Some(saved) = read_prefix_cache(data_dir, session_id) else {
            return;
        };
        if let Some(prefix) = saved.prefix {
            self.store_prefix(session_id, prefix);
        }
        self.store_request_messages(session_id, saved.messages);
    }

    pub fn persist_prefix_cache(&self, data_dir: &std::path::Path, session_id: &str) {
        let Some(messages) = self.last_request_messages(session_id) else {
            return;
        };
        write_prefix_cache(
            data_dir,
            session_id,
            &PrefixCacheFile {
                messages,
                prefix: self.last_prefix(session_id),
            },
        );
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PrefixCacheFile {
    messages: Vec<ChatMessage>,
    prefix: Option<RequestFingerprint>,
}

fn prefix_cache_path(data_dir: &std::path::Path, session_id: &str) -> std::path::PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == ':' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir.join("prompt-cache").join(format!("{safe}.json"))
}

fn read_prefix_cache(data_dir: &std::path::Path, session_id: &str) -> Option<PrefixCacheFile> {
    let path = prefix_cache_path(data_dir, session_id);
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_prefix_cache(data_dir: &std::path::Path, session_id: &str, saved: &PrefixCacheFile) {
    let path = prefix_cache_path(data_dir, session_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(saved) {
        let _ = std::fs::write(path, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_flags_default_and_set() {
        let s = SessionState::new();
        assert_eq!(s.safety_override("a"), None);
        assert!(!s.plan_approved("a"));
        s.set_full_auto("a");
        s.mark_plan_approved("a");
        assert_eq!(s.safety_override("a").as_deref(), Some("full"));
        assert!(s.plan_approved("a"));
        // Untouched session stays default.
        assert_eq!(s.safety_override("b"), None);
    }

    #[test]
    fn request_prefix_survives_across_lookups() {
        let s = SessionState::new();
        assert!(s.last_request_messages("chat").is_none());
        s.store_request_messages("chat", vec![ChatMessage::user("hi")]);
        let got = s.last_request_messages("chat").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].content, "hi");
        assert!(s.last_request_messages("other").is_none());
    }

    #[test]
    fn plan_approval_clears_without_resetting_safety_override() {
        let s = SessionState::new();
        s.set_full_auto("a");
        s.mark_plan_approved("a");
        assert!(s.plan_approved("a"));

        s.clear_plan_approved("a");

        assert!(!s.plan_approved("a"));
        assert_eq!(s.safety_override("a").as_deref(), Some("full"));
        assert!(!s.plan_approved("b"));
    }

    #[test]
    fn prompt_resolve_roundtrip() {
        let r = PromptRegistry::new();
        let mut rx = r.register("q1".into());
        assert!(r.resolve("q1", "hello".into()));
        assert_eq!(rx.try_recv().unwrap(), "hello");
        assert!(!r.resolve("q1", "again".into()));
    }
}
