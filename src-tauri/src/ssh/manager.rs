use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use base64::Engine;
use dashmap::DashMap;
use russh::client::Msg;
use russh::{Channel, ChannelMsg, Disconnect};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::client;
use crate::storage::models::Vps;
use crate::storage::{Db, HostKeyVerdict};

const RING_CAPACITY: usize = 256 * 1024; // bytes of scrollback replay buffer per session

/// Commands sent to a running session's I/O task.
enum SessionCmd {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Close,
}

/// Connection status reported to the UI. Some variants are produced by
/// reconnect/error paths driven from the frontend and the backend status events.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail")]
pub enum SessionStatus {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
    Error(String),
}

/// Capped FIFO byte buffer for instant replay on re-focus / reconnect.
struct RingBuffer {
    buf: VecDeque<u8>,
    cap: usize,
}

impl RingBuffer {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(1024),
            cap,
        }
    }
    fn push(&mut self, data: &[u8]) {
        self.buf.extend(data.iter().copied());
        while self.buf.len() > self.cap {
            self.buf.pop_front();
        }
    }
    fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }
}

struct SessionHandle {
    // Used by the broadcast / session-to-VPS mapping features.
    #[allow(dead_code)]
    vps_id: String,
    tx: tokio::sync::mpsc::UnboundedSender<SessionCmd>,
    ring: Arc<Mutex<RingBuffer>>,
    status: Arc<Mutex<SessionStatus>>,
}

/// Result of a connect call, returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectOutcome {
    pub session_id: String,
    pub vps_id: String,
    /// "match" | "pinned_on_first_use" (a host-key mismatch fails the connect,
    /// so this outcome is never produced with "mismatch").
    pub host_key: String,
}

#[derive(Clone)]
pub struct SessionManager {
    map: Arc<DashMap<String, SessionHandle>>,
    app: AppHandle,
    db: Db,
}

impl SessionManager {
    pub fn new(app: AppHandle, db: Db) -> Self {
        Self {
            map: Arc::new(DashMap::new()),
            app,
            db,
        }
    }

    fn event_output(session_id: &str) -> String {
        format!("ssh://{session_id}/output")
    }

    fn event_status(session_id: &str) -> String {
        format!("ssh://{session_id}/status")
    }

    fn set_status(&self, session_id: &str, status: SessionStatus) {
        if let Some(h) = self.map.get(session_id) {
            *h.status.lock().unwrap() = status.clone();
        }
        let _ = self.app.emit(&Self::event_status(session_id), status);
    }

    /// Establish a new interactive shell session for the given VPS.
    pub async fn connect(&self, vps: Vps, cols: u32, rows: u32) -> Result<ConnectOutcome, String> {
        let session_id = Uuid::new_v4().to_string();

        let auth = client::resolve_auth(&vps).map_err(|e| e.to_string())?;

        let connected = client::connect(&vps.host, vps.port, &vps.username, auth, self.db.clone())
            .await
            .map_err(|e| e.to_string())?;

        let host_key = match connected.verdict {
            HostKeyVerdict::Match => "match",
            HostKeyVerdict::PinnedOnFirstUse => "pinned_on_first_use",
            HostKeyVerdict::Mismatch { .. } => "mismatch",
        }
        .to_string();

        let handle = connected.handle;

        let channel: Channel<Msg> = handle
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;
        channel
            .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|e| e.to_string())?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| e.to_string())?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SessionCmd>();
        let ring = Arc::new(Mutex::new(RingBuffer::new(RING_CAPACITY)));
        let status = Arc::new(Mutex::new(SessionStatus::Connected));

        self.map.insert(
            session_id.clone(),
            SessionHandle {
                vps_id: vps.id.clone(),
                tx,
                ring: ring.clone(),
                status: status.clone(),
            },
        );

        let app = self.app.clone();
        let sid = session_id.clone();
        let map = self.map.clone();
        tokio::spawn(async move {
            run_session(handle, channel, rx, app.clone(), sid.clone(), ring).await;
            // Task ended: mark disconnected and drop the handle entry.
            *status.lock().unwrap() = SessionStatus::Disconnected;
            let _ = app.emit(&SessionManager::event_status(&sid), SessionStatus::Disconnected);
            map.remove(&sid);
        });

        self.set_status(&session_id, SessionStatus::Connected);

        Ok(ConnectOutcome {
            session_id,
            vps_id: vps.id,
            host_key,
        })
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let h = self
            .map
            .get(session_id)
            .ok_or_else(|| "session not found".to_string())?;
        h.tx
            .send(SessionCmd::Data(data.to_vec()))
            .map_err(|_| "session closed".to_string())
    }

    pub fn resize(&self, session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
        let h = self
            .map
            .get(session_id)
            .ok_or_else(|| "session not found".to_string())?;
        h.tx
            .send(SessionCmd::Resize { cols, rows })
            .map_err(|_| "session closed".to_string())
    }

    pub fn disconnect(&self, session_id: &str) -> Result<(), String> {
        if let Some(h) = self.map.get(session_id) {
            let _ = h.tx.send(SessionCmd::Close);
        }
        Ok(())
    }

    /// Tear down every live session. Used when the app re-locks: an unlocked app is what
    /// authorises a shell, so the shells must not outlive the unlock. Returns how many were
    /// closed, and clears the map — a locked app holds no session handles, and therefore no
    /// scrollback, in RAM.
    ///
    /// Scrollback is snapshotted by the caller *before* this, if it is being persisted; there
    /// is nothing left to read afterwards.
    pub fn disconnect_all(&self) -> usize {
        let ids: Vec<String> = self.map.iter().map(|e| e.key().clone()).collect();
        for id in &ids {
            let _ = self.disconnect(id);
        }
        self.map.clear();
        ids.len()
    }

    /// Base64 of the session's recent output, for replay on re-focus / reconnect.
    pub fn replay(&self, session_id: &str) -> Option<String> {
        self.map.get(session_id).and_then(|h| {
            let st = h.status.lock().unwrap().clone();
            if st != SessionStatus::Connected {
                return None;
            }
            Some(base64::engine::general_purpose::STANDARD.encode(h.ring.lock().unwrap().snapshot()))
        })
    }

    #[allow(dead_code)]
    pub fn vps_id_for(&self, session_id: &str) -> Option<String> {
        self.map.get(session_id).map(|h| h.vps_id.clone())
    }

    /// Live interactive session ids for a VPS (the terminals open on the canvas).
    /// Lets the agent drive the terminals the user is actually watching.
    pub fn live_sessions_for_vps(&self, vps_id: &str) -> Vec<String> {
        self.map
            .iter()
            .filter(|e| e.value().vps_id == vps_id)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Snapshot of a session's recent scrollback as plain text (ANSI stripped),
    /// so the agent can "read the screen" of a live terminal.
    pub fn capture_text(&self, session_id: &str) -> Option<String> {
        self.map.get(session_id).map(|h| {
            let bytes = h.ring.lock().unwrap().snapshot();
            strip_ansi(&String::from_utf8_lossy(&bytes))
        })
    }

    /// Run a single command on a VPS non-interactively and capture its output.
    ///
    /// Delegates to the shared headless path ([`super::command::run_vps_command`]) —
    /// the one command-execution implementation used by the agent, cron, MCP, and
    /// remote file ops — which already applies [`super::command::COMMAND_TIMEOUT`].
    pub async fn run_command(&self, vps_id: &str, command: &str) -> Result<CommandOutput, String> {
        super::command::run_vps_command(&self.db, vps_id, command).await
    }
}

#[cfg(test)]
mod tests {
    use super::{strip_ansi, RingBuffer, OUTPUT_FLUSH_BYTES};

    #[test]
    fn ring_keeps_everything_under_capacity() {
        let mut ring = RingBuffer::new(8);
        ring.push(b"abc");
        ring.push(b"de");
        assert_eq!(ring.snapshot(), b"abcde");
    }

    #[test]
    fn ring_drops_oldest_bytes_past_capacity() {
        let mut ring = RingBuffer::new(4);
        ring.push(b"abcdef");
        // Scrollback replay shows the most recent bytes, not the first ones.
        assert_eq!(ring.snapshot(), b"cdef");
        ring.push(b"gh");
        assert_eq!(ring.snapshot(), b"efgh");
    }

    #[test]
    fn ring_handles_a_single_push_larger_than_capacity() {
        let mut ring = RingBuffer::new(3);
        ring.push(b"0123456789");
        assert_eq!(ring.snapshot(), b"789");
    }

    #[test]
    fn ring_push_of_nothing_is_a_no_op() {
        let mut ring = RingBuffer::new(4);
        ring.push(b"ab");
        ring.push(b"");
        assert_eq!(ring.snapshot(), b"ab");
    }

    #[test]
    fn output_flush_threshold_is_below_the_ring_capacity() {
        // A burst big enough to force an immediate emit must still fit in the replay
        // ring, otherwise a reconnect right after one would replay a truncated screen.
        assert!(OUTPUT_FLUSH_BYTES < super::RING_CAPACITY);
    }

    #[test]
    fn strips_csi_and_keeps_text() {
        assert_eq!(strip_ansi("\u{1b}[31mhi\u{1b}[0m there"), "hi there");
        assert_eq!(strip_ansi("plain\nline"), "plain\nline");
    }

    #[test]
    fn strips_osc_title() {
        assert_eq!(strip_ansi("\u{1b}]0;my title\u{7}prompt$ "), "prompt$ ");
    }
}

/// Strip ANSI/VT escape sequences (CSI and OSC) from terminal output, keeping
/// printable text and newlines — used by `capture_text` so the agent reads clean text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    // CSI: parameter/intermediate bytes then a final letter.
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: terminated by BEL or ST (ESC \).
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Result of a non-interactive command execution.
#[derive(Debug, Clone, Serialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Longest an output byte may wait in the coalescing buffer before it is emitted.
///
/// russh hands us one `ChannelMsg::Data` per SSH packet, and a noisy command
/// (`tail -f`, a build, `cat` on a large file) produces hundreds per second. Emitting
/// each one separately means hundreds of base64 encodes, JSON serialisations, IPC
/// hops, `atob` decodes and `term.write` calls per second — the terminal's dominant
/// cost, and it is all per-message overhead rather than per-byte work.
///
/// One frame at 60 Hz is 16.7 ms, so an 8 ms window is invisible to someone typing
/// (their echo still lands well inside the same frame) while collapsing a firehose
/// into at most ~125 emits per second.
const OUTPUT_FLUSH: std::time::Duration = std::time::Duration::from_millis(8);

/// Flush as soon as this much output is pending, regardless of the timer. Bounds the
/// buffer against a producer faster than the window, and keeps the UI painting
/// steadily instead of receiving one enormous blob.
const OUTPUT_FLUSH_BYTES: usize = 64 * 1024;

fn emit_output(app: &AppHandle, session_id: &str, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let _ = app.emit(&SessionManager::event_output(session_id), b64);
}

async fn run_session(
    handle: russh::client::Handle<client::Handler>,
    mut channel: Channel<Msg>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<SessionCmd>,
    app: AppHandle,
    session_id: String,
    ring: Arc<Mutex<RingBuffer>>,
) {
    // Output waiting to be emitted, and when it must go out by. `flush_at` is armed
    // only while something is pending, so an idle session never wakes on a timer.
    let mut pending: Vec<u8> = Vec::new();
    let mut flush_at: Option<tokio::time::Instant> = None;

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(SessionCmd::Data(d)) => {
                        if channel.data(&d[..]).await.is_err() { break; }
                    }
                    Some(SessionCmd::Resize { cols, rows }) => {
                        let _ = channel.window_change(cols, rows, 0, 0).await;
                    }
                    Some(SessionCmd::Close) | None => {
                        let _ = channel.eof().await;
                        break;
                    }
                }
            }
            _ = async {
                match flush_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    // Nothing pending: never completes, so this branch stays inert.
                    None => std::future::pending::<()>().await,
                }
            } => {
                emit_output(&app, &session_id, &pending);
                pending.clear();
                flush_at = None;
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { ref data })
                    | Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                        // The replay ring is fed immediately, never on the flush
                        // schedule, so a reconnect or re-focus during the window
                        // still replays every byte the server sent.
                        ring.lock().unwrap().push(data);
                        pending.extend_from_slice(data);
                        if pending.len() >= OUTPUT_FLUSH_BYTES {
                            emit_output(&app, &session_id, &pending);
                            pending.clear();
                            flush_at = None;
                        } else if flush_at.is_none() {
                            flush_at = Some(tokio::time::Instant::now() + OUTPUT_FLUSH);
                        }
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    // The session ended with output still inside the coalescing window — a command
    // whose last line arrived just before EOF must not be lost.
    emit_output(&app, &session_id, &pending);
    let _ = handle
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
}
