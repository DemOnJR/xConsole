use std::sync::{Arc, Mutex};

use russh::client::{self, Handle};
use russh::keys::*;
use zeroize::Zeroizing;

use crate::secrets;
use crate::storage::models::{AuthType, Vps};
use crate::storage::{Db, HostKeyVerdict};

/// Errors that can occur while establishing an SSH session.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("host key mismatch (possible MITM); pinned fingerprint: {expected}")]
    HostKeyMismatch { expected: String },
    #[error("authentication failed")]
    AuthFailed,
    #[error("ssh-agent authentication is not supported in this build yet")]
    AgentUnsupported,
    #[error("missing credential: {0}")]
    MissingCredential(String),
    #[error("ssh error: {0}")]
    Ssh(#[from] russh::Error),
    #[error("key error: {0}")]
    Key(#[from] russh::keys::Error),
    #[error("{0}")]
    Other(String),
}

/// russh client handler. Performs trust-on-first-use host key verification and
/// records the verdict so the caller can surface "pinned on first use" to the UI.
pub struct Handler {
    db: Db,
    host: String,
    port: u16,
    verdict: Arc<Mutex<Option<HostKeyVerdict>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let key_type = server_public_key.algorithm().as_str().to_string();
        let fingerprint = server_public_key.fingerprint(Default::default()).to_string();

        match self
            .db
            .verify_host_key(&self.host, self.port, &key_type, &fingerprint)
        {
            Ok(verdict) => {
                let accept = !matches!(verdict, HostKeyVerdict::Mismatch { .. });
                *self.verdict.lock().unwrap() = Some(verdict);
                Ok(accept)
            }
            // Fail closed if we cannot consult the known_hosts store.
            Err(_) => Ok(false),
        }
    }
}

/// Outcome of a successful connection: the live handle plus the host-key verdict.
pub struct Connected {
    pub handle: Handle<Handler>,
    pub verdict: HostKeyVerdict,
}

/// Connect to a VPS and authenticate. The private key path, auth type, and any
/// keychain-stored secret are resolved by the caller via [`crate::ssh::Auth`].
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    auth: Auth,
    db: Db,
) -> Result<Connected, ConnectError> {
    let verdict_slot: Arc<Mutex<Option<HostKeyVerdict>>> = Arc::new(Mutex::new(None));
    let handler = Handler {
        db,
        host: host.to_string(),
        port,
        verdict: verdict_slot.clone(),
    };

    let config = Arc::new(client::Config {
        // We manage liveness/reconnect ourselves; don't drop idle interactive shells.
        inactivity_timeout: None,
        ..Default::default()
    });

    let connect_res = client::connect(config, (host, port), handler).await;
    let mut handle = match connect_res {
        Ok(h) => h,
        Err(e) => {
            // If the failure was a host-key mismatch, surface that specifically.
            if let Some(HostKeyVerdict::Mismatch { expected }) =
                verdict_slot.lock().unwrap().clone()
            {
                return Err(ConnectError::HostKeyMismatch { expected });
            }
            return Err(ConnectError::Ssh(e));
        }
    };

    let authed = match auth {
        Auth::Password(password) => handle
            .authenticate_password(username, password)
            .await?
            .success(),
        Auth::Key { source, passphrase } => {
            let key = match source {
                KeySource::Pem(pem) => decode_secret_key(&pem, passphrase.as_deref())?,
                KeySource::Path(path) => load_secret_key(&path, passphrase.as_deref())?,
            };
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            handle
                .authenticate_publickey(
                    username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await?
                .success()
        }
    };

    if !authed {
        return Err(ConnectError::AuthFailed);
    }

    let verdict = verdict_slot
        .lock()
        .unwrap()
        .clone()
        .unwrap_or(HostKeyVerdict::Match);

    Ok(Connected { handle, verdict })
}

/// Resolved authentication material for a single connection attempt.
pub enum Auth {
    Password(String),
    Key {
        source: KeySource,
        passphrase: Option<String>,
    },
}

/// Where a private key's bytes come from: app-managed (PEM held in the OS
/// keychain, never on disk) or a user-provided key file referenced by path.
pub enum KeySource {
    /// Inline OpenSSH PEM (wiped from memory on drop).
    Pem(Zeroizing<String>),
    /// Path to a private-key file on disk.
    Path(String),
}

/// Build authentication material for a VPS from its stored auth type plus any
/// OS-keychain secret. The single source of truth for the SSH/SFTP/command paths.
pub fn resolve_auth(vps: &Vps) -> Result<Auth, ConnectError> {
    match vps.auth_type {
        // ssh-agent auth isn't wired up yet; fail before any network I/O or
        // host-key pinning rather than after a wasted handshake.
        AuthType::Agent => Err(ConnectError::AgentUnsupported),
        AuthType::Password => {
            let secret = secrets::get_secret(&vps.id)
                .map_err(|e| ConnectError::Other(e.to_string()))?
                .ok_or_else(|| ConnectError::MissingCredential("password".into()))?;
            Ok(Auth::Password(secret.to_string()))
        }
        AuthType::Key => {
            // Prefer an app-managed key held in the keychain (no disk footprint).
            // The passphrase secret under `vps.id` only applies to a key *file*;
            // app-managed keys are generated without a passphrase.
            if let Some(pem) = secrets::get_secret(&secrets::ssh_key_key(&vps.id))
                .map_err(|e| ConnectError::Other(e.to_string()))?
            {
                return Ok(Auth::Key {
                    source: KeySource::Pem(pem),
                    passphrase: None,
                });
            }
            let key_path = vps
                .key_path
                .clone()
                .ok_or_else(|| ConnectError::MissingCredential("key_path".into()))?;
            let passphrase = secrets::get_secret(&vps.id)
                .map_err(|e| ConnectError::Other(e.to_string()))?
                .map(|z| z.to_string());
            Ok(Auth::Key {
                source: KeySource::Path(key_path),
                passphrase,
            })
        }
    }
}
