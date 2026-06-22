use std::sync::{Arc, Mutex};

use russh::client::{self, Handle};
use russh::keys::*;

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
        Auth::Agent => return Err(ConnectError::AgentUnsupported),
        Auth::Password(password) => handle
            .authenticate_password(username, password)
            .await?
            .success(),
        Auth::Key {
            key_path,
            passphrase,
        } => {
            let key = load_secret_key(&key_path, passphrase.as_deref())?;
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
    Agent,
    Password(String),
    Key {
        key_path: String,
        passphrase: Option<String>,
    },
}
