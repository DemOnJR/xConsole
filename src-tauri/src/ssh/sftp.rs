//! Persistent SFTP browser sessions (separate from interactive shell sessions).
//!
//! # One connection, many channels
//!
//! A session owns the SSH [`Handle`] itself rather than parking it in a task. russh's
//! `connect` already spawns the task that drives the connection — `Handle` merely holds
//! its `JoinHandle`, and `impl Future for Handle` only *waits for the session to end*.
//! Awaiting it therefore buys nothing and costs everything: the handle is moved away
//! and no further channel can ever be opened on that connection.
//!
//! Keeping it (as an `Arc`, since `channel_open_session` takes `&self`) is what makes
//! parallel transfers possible — each worker gets its own SFTP channel instead of
//! queueing behind one mutex — and lets the archive path run `tar` over an exec channel
//! on the same authenticated connection, with no second handshake.

use std::sync::Arc;

use base64::Engine;
use dashmap::DashMap;
use russh::client::Handle;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::client::{self, Handler};
use crate::storage::Db;

/// Ceiling for the in-memory read/write path (`download`/`write`), which round-trips
/// the whole body through base64 over IPC. Bulk transfers stream to disk instead and
/// are not bounded by this — see [`super::transfer`].
const MAX_DOWNLOAD: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct SftpConnectOutcome {
    pub session_id: String,
    pub vps_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpListOutcome {
    pub path: String,
    pub entries: Vec<SftpEntry>,
}

struct SftpHandle {
    /// The live SSH connection. Kept so extra SFTP/exec channels can be opened on it.
    ssh: Arc<Handle<Handler>>,
    /// The browser's own channel, used for listing, stat and small reads/writes.
    sftp: Arc<Mutex<SftpSession>>,
    /// Which server this session belongs to (for remote-command helpers).
    vps_id: String,
}

#[derive(Clone)]
pub struct SftpManager {
    map: Arc<DashMap<String, SftpHandle>>,
    db: Db,
}

/// A session's reusable pieces, handed to the transfer engine. Deliberately does not
/// expose the browser's own channel: transfers open their own so they never block
/// listing, which is the whole point of keeping the SSH handle around.
pub struct SessionRefs {
    pub ssh: Arc<Handle<Handler>>,
    pub vps_id: String,
}

impl SftpManager {
    pub fn new(db: Db) -> Self {
        Self {
            map: Arc::new(DashMap::new()),
            db,
        }
    }

    pub async fn connect(&self, vps_id: &str) -> Result<SftpConnectOutcome, String> {
        let vps = self
            .db
            .get_vps(vps_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "VPS not found".to_string())?;

        let auth = client::resolve_auth(&vps).map_err(|e| e.to_string())?;
        let connected = client::connect(&vps.host, vps.port, &vps.username, auth, self.db.clone())
            .await
            .map_err(|e| e.to_string())?;

        let handle = connected.handle;
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("SFTP subsystem unavailable: {e}"))?;

        let stream = channel.into_stream();
        let sftp = SftpSession::new(stream)
            .await
            .map_err(|e| format!("SFTP init failed: {e}"))?;

        let session_id = Uuid::new_v4().to_string();
        let start_path = sftp
            .canonicalize(".")
            .await
            .unwrap_or_else(|_| "/".to_string());

        self.map.insert(
            session_id.clone(),
            SftpHandle {
                ssh: Arc::new(handle),
                sftp: Arc::new(Mutex::new(sftp)),
                vps_id: vps.id.clone(),
            },
        );

        Ok(SftpConnectOutcome {
            session_id,
            vps_id: vps.id,
            path: start_path,
        })
    }

    pub async fn list(&self, session_id: &str, path: &str) -> Result<SftpListOutcome, String> {
        let path = normalize_path(path);
        let entry = self
            .map
            .get(session_id)
            .ok_or_else(|| "SFTP session not found".to_string())?;
        let sftp = entry.sftp.clone();
        drop(entry);

        let sftp = sftp.lock().await;
        let dir = sftp
            .read_dir(&path)
            .await
            .map_err(|e| format!("list failed: {e}"))?;

        let mut entries = Vec::new();
        for item in dir {
            let name = item.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let meta = item.metadata();
            let is_dir = meta.is_dir();
            let size = meta.size.unwrap_or(0);
            let child = join_path(&path, &name);
            entries.push(SftpEntry {
                name,
                path: child,
                is_dir,
                size,
            });
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));

        Ok(SftpListOutcome { path, entries })
    }

    pub async fn download(&self, session_id: &str, path: &str) -> Result<String, String> {
        let path = normalize_path(path);
        let entry = self
            .map
            .get(session_id)
            .ok_or_else(|| "SFTP session not found".to_string())?;
        let sftp = entry.sftp.clone();
        drop(entry);

        let sftp = sftp.lock().await;
        let meta = sftp
            .metadata(&path)
            .await
            .map_err(|e| format!("stat failed: {e}"))?;
        if meta.is_dir() {
            return Err("cannot download a directory".into());
        }
        let size = meta.size.unwrap_or(0);
        if size > MAX_DOWNLOAD {
            return Err(format!("file too large ({size} bytes, max {MAX_DOWNLOAD})"));
        }

        let file = sftp
            .open(&path)
            .await
            .map_err(|e| format!("open failed: {e}"))?;
        let mut buf = Vec::new();
        use tokio::io::AsyncReadExt;
        // Cap the actual read, not just the reported size: some servers report
        // size 0 for special files, which would otherwise read unbounded.
        file.take(MAX_DOWNLOAD + 1)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        if buf.len() as u64 > MAX_DOWNLOAD {
            return Err(format!("file too large (max {MAX_DOWNLOAD} bytes)"));
        }
        Ok(base64::engine::general_purpose::STANDARD.encode(buf))
    }

    /// Overwrite (or create) a remote file with `content_b64` (base64), atomically.
    pub async fn write(&self, session_id: &str, path: &str, content_b64: &str) -> Result<(), String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content_b64.as_bytes())
            .map_err(|e| format!("invalid base64: {e}"))?;
        if bytes.len() as u64 > MAX_DOWNLOAD {
            return Err(format!("file too large ({} bytes, max {MAX_DOWNLOAD})", bytes.len()));
        }

        let path = normalize_path(path);
        let entry = self
            .map
            .get(session_id)
            .ok_or_else(|| "SFTP session not found".to_string())?;
        let sftp = entry.sftp.clone();
        drop(entry);

        let sftp = sftp.lock().await;
        write_atomic(&sftp, &path, &bytes).await
    }

    pub fn disconnect(&self, session_id: &str) -> Result<(), String> {
        // Dropping the entry drops the last `Arc<Handle>` this manager holds; russh
        // tears the session down once every clone is gone (a transfer still running on
        // its own clone finishes first rather than being cut off mid-file).
        self.map.remove(session_id);
        Ok(())
    }

    /// The database handle, for helpers that need to run a remote command.
    pub fn db_ref(&self) -> &Db {
        &self.db
    }

    /// The reusable parts of a session, for the transfer engine.
    pub fn session_refs(&self, session_id: &str) -> Result<SessionRefs, String> {
        let entry = self
            .map
            .get(session_id)
            .ok_or_else(|| "SFTP session not found".to_string())?;
        Ok(SessionRefs {
            ssh: entry.ssh.clone(),
            vps_id: entry.vps_id.clone(),
        })
    }
}

/// Open an additional SFTP channel on an existing connection.
///
/// Each concurrent transfer worker gets its own channel so they genuinely overlap;
/// sharing the browser's single channel would serialise them behind its mutex and make
/// the concurrency setting meaningless.
pub async fn open_channel(ssh: &Handle<Handler>) -> Result<SftpSession, String> {
    let channel = ssh
        .channel_open_session()
        .await
        .map_err(|e| format!("open channel failed: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("SFTP subsystem unavailable: {e}"))?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP init failed: {e}"))
}

/// Write `bytes` to `path` without ever leaving a partial file behind.
///
/// The previous implementation opened the destination with `TRUNCATE` and then wrote
/// into it, which **destroys the file before the first byte lands**. Any failure in
/// between — dropped connection, full disk, a per-request timeout — left a truncated or
/// zero-byte file and no way back. That is the classic "saved my config and it came
/// back empty" failure, and it is why editors that save over SFTP naively can lose
/// files.
///
/// Instead: write a sibling temp file, verify the server agrees on its size, then
/// rename over the target. Rename is atomic on POSIX, so a reader sees either the old
/// file or the new one, never an empty one. The size check is what makes a short write
/// a *failed save* instead of a silent truncation — the original stays untouched.
pub async fn write_atomic(sftp: &SftpSession, path: &str, bytes: &[u8]) -> Result<(), String> {
    use russh_sftp::protocol::OpenFlags;
    use tokio::io::AsyncWriteExt;

    if sftp.metadata(path).await.map(|m| m.is_dir()).unwrap_or(false) {
        return Err("cannot write to a directory".into());
    }

    // Same directory, so the rename stays on one filesystem and preserves the
    // destination's ownership semantics.
    let tmp = format!("{path}.xconsole-{}.tmp", Uuid::new_v4().simple());

    let result = async {
        let mut file = sftp
            .open_with_flags(
                &tmp,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
            )
            .await
            .map_err(|e| format!("open for write failed: {e}"))?;
        file.write_all(bytes)
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        file.shutdown()
            .await
            .map_err(|e| format!("flush failed: {e}"))?;
        drop(file);

        // Confirm the server really stored everything before anything is overwritten.
        let wrote = sftp
            .metadata(&tmp)
            .await
            .map_err(|e| format!("could not verify the upload: {e}"))?
            .size
            .unwrap_or(0);
        if wrote != bytes.len() as u64 {
            return Err(format!(
                "short write — sent {} bytes but the server stored {wrote}; \
                 the original file was left untouched",
                bytes.len()
            ));
        }
        Ok(())
    }
    .await;

    if let Err(e) = result {
        let _ = sftp.remove_file(&tmp).await;
        return Err(e);
    }

    // `rename` fails on most servers when the target exists, so try the atomic
    // POSIX-extension rename first and fall back to unlink+rename.
    if sftp.rename(&tmp, path).await.is_err() {
        let _ = sftp.remove_file(path).await;
        if let Err(e) = sftp.rename(&tmp, path).await {
            let _ = sftp.remove_file(&tmp).await;
            return Err(format!("could not replace the file: {e}"));
        }
    }
    Ok(())
}

fn normalize_path(path: &str) -> String {
    let p = path.trim();
    if p.is_empty() || p == "." {
        return "/".to_string();
    }
    if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    }
}

fn join_path(base: &str, name: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        format!("/{name}")
    } else {
        format!("{base}/{name}")
    }
}

