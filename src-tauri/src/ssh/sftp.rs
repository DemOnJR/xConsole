//! Persistent SFTP browser sessions (separate from interactive shell sessions).

use std::sync::Arc;

use base64::Engine;
use dashmap::DashMap;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::client;
use crate::storage::Db;

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
    sftp: Arc<Mutex<SftpSession>>,
    pump: JoinHandle<()>,
}

#[derive(Clone)]
pub struct SftpManager {
    map: Arc<DashMap<String, SftpHandle>>,
    db: Db,
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

        let pump = tokio::spawn(async move {
            let _ = handle.await;
        });

        let session_id = Uuid::new_v4().to_string();
        let start_path = sftp
            .canonicalize(".")
            .await
            .unwrap_or_else(|_| "/".to_string());

        self.map.insert(
            session_id.clone(),
            SftpHandle {
                sftp: Arc::new(Mutex::new(sftp)),
                pump,
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

    /// Overwrite (or create) a remote file with `content_b64` (base64). Opens with
    /// CREATE|TRUNCATE so a shorter new body fully replaces the old one.
    pub async fn write(&self, session_id: &str, path: &str, content_b64: &str) -> Result<(), String> {
        use russh_sftp::protocol::OpenFlags;
        use tokio::io::AsyncWriteExt;

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
        if sftp.metadata(&path).await.map(|m| m.is_dir()).unwrap_or(false) {
            return Err("cannot write to a directory".into());
        }
        let mut file = sftp
            .open_with_flags(&path, OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE)
            .await
            .map_err(|e| format!("open for write failed: {e}"))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        file.shutdown()
            .await
            .map_err(|e| format!("flush failed: {e}"))?;
        Ok(())
    }

    pub fn disconnect(&self, session_id: &str) -> Result<(), String> {
        if let Some((_, handle)) = self.map.remove(session_id) {
            handle.pump.abort();
        }
        Ok(())
    }
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

