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

/// Tracks in-flight `ask_user` / `present_plan` prompts so the UI can resolve
/// them with the user's answer. Managed Tauri state.
#[derive(Clone, Default)]
pub struct PromptRegistry {
    pending: Arc<DashMap<String, oneshot::Sender<String>>>,
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

    /// Deliver the user's answer to a waiting prompt. Returns true if it was awaiting.
    pub fn resolve(&self, id: &str, answer: String) -> bool {
        if let Some((_, tx)) = self.pending.remove(id) {
            let _ = tx.send(answer);
            true
        } else {
            false
        }
    }

    /// Drop a pending prompt without answering (e.g. on timeout).
    pub fn cancel(&self, id: &str) -> bool {
        self.pending.remove(id).is_some()
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
    fn prompt_resolve_roundtrip() {
        let r = PromptRegistry::new();
        let mut rx = r.register("q1".into());
        assert!(r.resolve("q1", "hello".into()));
        assert_eq!(rx.try_recv().unwrap(), "hello");
        assert!(!r.resolve("q1", "again".into()));
    }
}
