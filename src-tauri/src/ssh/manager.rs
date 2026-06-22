use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use base64::Engine;
use dashmap::DashMap;
use russh::client::Msg;
use russh::{Channel, ChannelMsg, Disconnect};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::client::{self, Auth, ConnectError};
use crate::secrets;
use crate::storage::models::{AuthType, Vps};
use crate::storage::{Db, HostKeyVerdict};

const RING_CAPACITY: usize = 256 * 1024; // bytes of scrollback replay buffer per session

/// Maximum wall-clock time for a non-interactive SSH command (agent/cron).
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

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
    /// "match" | "pinned_on_first_use" | "mismatch"
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

    /// Build authentication material from the stored VPS + OS keychain.
    fn resolve_auth(&self, vps: &Vps) -> Result<Auth, ConnectError> {
        match vps.auth_type {
            AuthType::Agent => Ok(Auth::Agent),
            AuthType::Password => {
                let secret = secrets::get_secret(&vps.id)
                    .map_err(|e| ConnectError::Other(e.to_string()))?
                    .ok_or_else(|| ConnectError::MissingCredential("password".into()))?;
                Ok(Auth::Password(secret.to_string()))
            }
            AuthType::Key => {
                let key_path = vps
                    .key_path
                    .clone()
                    .ok_or_else(|| ConnectError::MissingCredential("key_path".into()))?;
                let passphrase = secrets::get_secret(&vps.id)
                    .map_err(|e| ConnectError::Other(e.to_string()))?
                    .map(|z| z.to_string());
                Ok(Auth::Key {
                    key_path,
                    passphrase,
                })
            }
        }
    }

    /// Establish a new interactive shell session for the given VPS.
    pub async fn connect(&self, vps: Vps, cols: u32, rows: u32) -> Result<ConnectOutcome, String> {
        let session_id = Uuid::new_v4().to_string();

        let auth = self.resolve_auth(&vps).map_err(|e| e.to_string())?;

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

    /// Base64 of the session's recent output, for replay on re-focus / reconnect.
    pub fn replay(&self, session_id: &str) -> Option<String> {
        self.map
            .get(session_id)
            .map(|h| base64::engine::general_purpose::STANDARD.encode(h.ring.lock().unwrap().snapshot()))
    }

    #[allow(dead_code)]
    pub fn vps_id_for(&self, session_id: &str) -> Option<String> {
        self.map.get(session_id).map(|h| h.vps_id.clone())
    }

    /// Run a single command on a VPS non-interactively and capture its output.
    ///
    /// This is the one command-execution path used by the agent, cron, and any
    /// future UI. It opens a dedicated connection (reusing the same auth + host
    /// key verification as interactive shells), runs the command, and returns
    /// stdout/stderr/exit code.
    pub async fn run_command(
        &self,
        vps_id: &str,
        command: &str,
    ) -> Result<CommandOutput, String> {
        match tokio::time::timeout(
            COMMAND_TIMEOUT,
            self.run_command_inner(vps_id, command),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => Err(format!(
                "command timed out after {}s",
                COMMAND_TIMEOUT.as_secs()
            )),
        }
    }

    async fn run_command_inner(
        &self,
        vps_id: &str,
        command: &str,
    ) -> Result<CommandOutput, String> {
        let vps = self
            .db
            .get_vps(vps_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "VPS not found".to_string())?;

        let auth = self.resolve_auth(&vps).map_err(|e| e.to_string())?;
        let connected =
            client::connect(&vps.host, vps.port, &vps.username, auth, self.db.clone())
                .await
                .map_err(|e| e.to_string())?;
        let handle = connected.handle;

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;
        channel.exec(true, command).await.map_err(|e| e.to_string())?;

        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut exit_code: Option<i32> = None;

        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { ref data }) => stdout.extend_from_slice(data),
                Some(ChannelMsg::ExtendedData { ref data, ext }) => {
                    if ext == 1 {
                        stderr.extend_from_slice(data);
                    } else {
                        stdout.extend_from_slice(data);
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    exit_code = Some(exit_status as i32);
                }
                Some(ChannelMsg::Eof) => {
                    if exit_code.is_some() {
                        break;
                    }
                }
                Some(ChannelMsg::Close) | None => break,
                _ => {}
            }
        }

        let _ = handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code: exit_code.unwrap_or(-1),
        })
    }
}

/// Result of a non-interactive command execution.
#[derive(Debug, Clone, Serialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

fn emit_output(
    app: &AppHandle,
    session_id: &str,
    data: &[u8],
    ring: &Arc<Mutex<RingBuffer>>,
) {
    ring.lock().unwrap().push(data);
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
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { ref data }) => {
                        emit_output(&app, &session_id, data, &ring);
                    }
                    Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                        emit_output(&app, &session_id, data, &ring);
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    let _ = handle
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
}
