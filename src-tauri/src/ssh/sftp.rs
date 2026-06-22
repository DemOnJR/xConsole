//! Persistent SFTP browser sessions (separate from interactive shell sessions).

use std::sync::Arc;

use base64::Engine;
use dashmap::DashMap;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::client::{self, Auth, ConnectError};
use crate::secrets;
use crate::storage::models::{AuthType, Vps};
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
    vps_id: String,
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

        let auth = resolve_auth(&vps).map_err(|e| e.to_string())?;
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
                vps_id: vps.id.clone(),
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

        let mut file = sftp
            .open(&path)
            .await
            .map_err(|e| format!("open failed: {e}"))?;
        let mut buf = Vec::new();
        use tokio::io::AsyncReadExt;
        file.read_to_end(&mut buf)
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(buf))
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

fn resolve_auth(vps: &Vps) -> Result<Auth, ConnectError> {
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
