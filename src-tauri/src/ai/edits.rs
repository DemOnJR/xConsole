//! In-session **edit journal**: tracks the files the agent writes (capturing the
//! content before and after each write) so the UI can show a GitHub-style diff
//! panel and offer one-click revert — without git.
//!
//! This is deliberately *not* git. For the "what did the agent just change?" view,
//! capturing before/after at the moment of the edit is faster and simpler: it's
//! O(1) per edit, needs no repository, and works identically for local files and
//! remote VPS files (where there is usually no git repo at all). We keep the
//! previous content, so revert is just writing it back.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Cap stored content per side so a runaway write can't balloon memory.
const MAX_CONTENT: usize = 256 * 1024;

static COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
pub struct EditRecord {
    pub id: String,
    pub session_id: String,
    /// "local" (this PC) | "vps" (a remote server).
    pub scope: String,
    pub vps_id: Option<String>,
    /// Human label: server "name (host)" or "This PC".
    pub label: String,
    pub path: String,
    pub before: String,
    pub after: String,
    /// The file did not exist before this edit (a creation).
    pub is_new: bool,
    pub reverted: bool,
    /// Unix epoch milliseconds.
    pub ts: i64,
}

/// Per-session journal of agent file edits. Cheap to clone (shared `Arc`).
#[derive(Clone, Default)]
pub struct EditJournal {
    inner: Arc<Mutex<HashMap<String, Vec<EditRecord>>>>,
}

impl EditJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one edit and notify the UI (`agent://file-change`).
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        app: &AppHandle,
        session_id: &str,
        scope: &str,
        vps_id: Option<String>,
        label: &str,
        path: &str,
        before: &str,
        after: &str,
    ) {
        // No-op when nothing actually changed (avoid noise from idempotent writes).
        if before == after {
            return;
        }
        let rec = EditRecord {
            id: format!("edit-{}", COUNTER.fetch_add(1, Ordering::Relaxed)),
            session_id: session_id.to_string(),
            scope: scope.to_string(),
            vps_id,
            label: label.to_string(),
            path: path.to_string(),
            before: clamp(before),
            after: clamp(after),
            is_new: before.is_empty(),
            reverted: false,
            ts: now_ms(),
        };
        if let Ok(mut map) = self.inner.lock() {
            map.entry(session_id.to_string())
                .or_default()
                .push(rec.clone());
        }
        let _ = app.emit("agent://file-change", &rec);
    }

    pub fn list(&self, session_id: &str) -> Vec<EditRecord> {
        self.inner
            .lock()
            .ok()
            .and_then(|m| m.get(session_id).cloned())
            .unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> Option<EditRecord> {
        let map = self.inner.lock().ok()?;
        map.values()
            .flat_map(|recs| recs.iter())
            .find(|r| r.id == id)
            .cloned()
    }

    pub fn mark_reverted(&self, id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            for recs in map.values_mut() {
                if let Some(r) = recs.iter_mut().find(|r| r.id == id) {
                    r.reverted = true;
                }
            }
        }
    }

    pub fn clear(&self, session_id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(session_id);
        }
    }
}

fn clamp(s: &str) -> String {
    if s.len() <= MAX_CONTENT {
        return s.to_string();
    }
    let mut cut = MAX_CONTENT;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n…(truncated)", &s[..cut])
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
