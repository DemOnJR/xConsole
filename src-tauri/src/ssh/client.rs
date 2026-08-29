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
    /// Deliberately spells out both fingerprints and what to do. The old message gave
    /// only the pinned one, which is the single value that cannot help: to decide whether
    /// the server was rebuilt or is being impersonated you have to compare what it is
    /// offering *now* against what you can see from somewhere else.
    #[error(
        "host key mismatch for this server's {key_type} key.\r\n\
         \x20 pinned  : {expected}\r\n\
         \x20 offered : {offered}\r\n\
         If you rebuilt or reinstalled this server, this is expected — its keys changed. \
         Check the new fingerprint on the server itself with `ssh-keygen -lf \
         /etc/ssh/ssh_host_{key_type}_key.pub` (or your provider's console), and if it \
         matches the offered value, forget the old key in Settings > Security > Pinned \
         host keys and connect again. If it does not match, do not connect: something is \
         answering for this address that is not your server."
    )]
    HostKeyMismatch {
        expected: String,
        offered: String,
        key_type: String,
    },
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

/// Where channels opened by the *server* should be piped, as a loopback port on this
/// machine.
///
/// Shared with the caller rather than fixed at connect time: a reverse forward is set up
/// long after the connection is authenticated, and only for the runs that need one.
/// `None` means the session has asked for no forward, so an unexpected forwarded channel
/// is refused instead of being wired to whatever port happens to be listening.
pub type ForwardTarget = Arc<Mutex<Option<u16>>>;

/// russh client handler. Performs trust-on-first-use host key verification and
/// records the verdict so the caller can surface "pinned on first use" to the UI.
pub struct Handler {
    db: Db,
    host: String,
    port: u16,
    verdict: Arc<Mutex<Option<HostKeyVerdict>>>,
    forward_target: ForwardTarget,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    /// A connection the remote side accepted on a reverse forward.
    ///
    /// Only ever expected while a remote agent run is in flight, which is the only time
    /// `forward_target` is set. Anything arriving outside that window is dropped: the
    /// channel is closed and nothing on this machine is contacted.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(port) = *self.forward_target.lock().unwrap() else {
            // Nothing asked for this. Dropping the channel closes it.
            return Ok(());
        };

        tokio::spawn(async move {
            let Ok(mut local) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else {
                return;
            };
            let mut remote = channel.into_stream();
            // Either side closing is the normal end of a forwarded connection.
            let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
        });
        Ok(())
    }

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
    /// Set this to a loopback port to accept the channels a reverse forward opens.
    /// See [`ForwardTarget`].
    pub forward_target: ForwardTarget,
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
    let forward_target: ForwardTarget = Arc::new(Mutex::new(None));
    let handler = Handler {
        db,
        host: host.to_string(),
        port,
        verdict: verdict_slot.clone(),
        forward_target: forward_target.clone(),
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
            if let Some(HostKeyVerdict::Mismatch {
                expected,
                offered,
                key_type,
            }) = verdict_slot.lock().unwrap().clone()
            {
                return Err(ConnectError::HostKeyMismatch {
                    expected,
                    offered,
                    key_type,
                });
            }
            // Name what we failed to reach. "ssh error: connection refused" leaves the
            // reader guessing which address and port were actually tried, which is the
            // whole question when the same command works in a terminal.
            return Err(ConnectError::Other(format!(
                "could not reach {host}:{port} — {e}"
            )));
        }
    };

    let authed = match auth {
        Auth::Password(password) => {
            // `password` first, then `keyboard-interactive` with the same secret.
            //
            // Only `password` was ever sent, and that is a smaller set of servers than
            // `ssh` reaches. A server with `PasswordAuthentication no` and
            // `KbdInteractiveAuthentication yes` — the default on several distributions
            // and most container images once PAM is in play — refuses the first and
            // accepts the second, so `ssh user@host` succeeds at a password prompt while
            // this returned "authentication failed" for the same credentials.
            if handle
                .authenticate_password(username, password.clone())
                .await?
                .success()
            {
                true
            } else {
                keyboard_interactive(&mut handle, username, &password).await?
            }
        }
        Auth::Key { source, passphrase } => {
            let key = match source {
                KeySource::Pem(pem) => decode_private_key(&pem, passphrase.as_deref())?,
                KeySource::Path(path) => load_private_key_file(&path, passphrase.as_deref())?,
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

    Ok(Connected { handle, verdict, forward_target })
}

/// How many info-request rounds a keyboard-interactive exchange may take.
///
/// The protocol lets a server send any number, including empty ones, so an unbounded loop
/// here is a hang waiting for a server that never says yes or no.
const MAX_KBD_ROUNDS: usize = 8;

/// Answers for one round of keyboard-interactive prompts.
///
/// A hidden prompt is a password prompt — that is what `echo: false` means, and it is what
/// PAM sends for "Password:". A prompt the server wants *echoed* is asking for something
/// visible: a one-time code, a second username, an acknowledgement. Sending the stored
/// password in reply to one of those would hand it to a field that is not a password
/// field, and it would be wrong anyway, so those get an empty answer.
fn kbd_answers(prompts: &[client::Prompt], password: &str) -> Vec<String> {
    prompts
        .iter()
        .map(|p| {
            if p.echo {
                String::new()
            } else {
                password.to_string()
            }
        })
        .collect()
}

/// Authenticate with `keyboard-interactive`, answering password prompts with `password`.
async fn keyboard_interactive(
    handle: &mut Handle<Handler>,
    username: &str,
    password: &str,
) -> Result<bool, ConnectError> {
    use client::KeyboardInteractiveAuthResponse as Resp;

    let mut resp = handle
        .authenticate_keyboard_interactive_start(username, None)
        .await?;
    for _ in 0..MAX_KBD_ROUNDS {
        match resp {
            Resp::Success => return Ok(true),
            Resp::Failure { .. } => return Ok(false),
            Resp::InfoRequest { ref prompts, .. } => {
                let answers = kbd_answers(prompts, password);
                resp = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await?;
            }
        }
    }
    Ok(false)
}

/// True for a PuTTY `.ppk`, by content rather than by file extension — the extension is a
/// convention and people rename things.
pub fn is_ppk(text: &str) -> bool {
    text.trim_start().starts_with("PuTTY-User-Key-File-")
}

/// Decode private key text in any format we accept, with an error worth reading.
///
/// PuTTY `.ppk` files work here and always have: russh routes anything starting with
/// `PuTTY-User-Key-File-` to the PPK parser, and it enables that support unconditionally.
/// What was missing was any acknowledgement of it — the field said `id_ed25519`, and a
/// `.ppk` that failed for an ordinary reason (a passphrase typo) reported a generic key
/// error that read like "this format is not supported". So a PPK failure now says it is a
/// PPK and lists what actually goes wrong with them.
pub fn decode_private_key(
    text: &str,
    passphrase: Option<&str>,
) -> Result<PrivateKey, ConnectError> {
    decode_secret_key(text, passphrase).map_err(|e| {
        if !is_ppk(text) {
            return ConnectError::Key(e);
        }
        ConnectError::Other(format!(
            "this PuTTY key (.ppk) could not be read: {e}. \
             If it is passphrase-protected, the passphrase goes in the key-passphrase \
             field. Note that DSA keys and the obsolete PPK version 1 are not supported — \
             convert those with PuTTYgen (Conversions > Export OpenSSH key)."
        ))
    })
}

/// Read a private key file from disk and decode it. Accepts OpenSSH, PEM and PuTTY `.ppk`.
pub fn load_private_key_file(
    path: &str,
    passphrase: Option<&str>,
) -> Result<PrivateKey, ConnectError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        ConnectError::Other(format!("could not read the private key at {path}: {e}"))
    })?;
    decode_private_key(&text, passphrase)
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

#[cfg(test)]
mod keyboard_interactive_tests {
    use super::kbd_answers;
    use russh::client::Prompt;

    fn p(prompt: &str, echo: bool) -> Prompt {
        Prompt { prompt: prompt.to_string(), echo }
    }

    /// The case that makes this worth having: PAM asks "Password:" with echo off.
    #[test]
    fn a_hidden_prompt_gets_the_password() {
        assert_eq!(kbd_answers(&[p("Password: ", false)], "hunter2"), vec!["hunter2"]);
    }

    /// An echoed prompt is asking for something visible — a one-time code, a second
    /// username. Answering it with the stored password would hand the password to a field
    /// that is not a password field, and would be the wrong answer besides.
    #[test]
    fn an_echoed_prompt_never_receives_the_password() {
        let answers = kbd_answers(&[p("Verification code: ", true)], "hunter2");
        assert_eq!(answers, vec![""]);
        assert!(!answers.iter().any(|a| a.contains("hunter2")));
    }

    /// Two-factor setups send both in one round, and the count of answers must match the
    /// count of prompts or the server rejects the response outright.
    #[test]
    fn a_mixed_round_answers_every_prompt_in_order() {
        let answers = kbd_answers(
            &[p("Password: ", false), p("One-time code: ", true)],
            "hunter2",
        );
        assert_eq!(answers, vec!["hunter2".to_string(), String::new()]);
    }

    /// A server may send an info request with no prompts at all — a banner. That is a
    /// valid round and needs an empty response, not a skipped one.
    #[test]
    fn a_prompt_less_round_produces_an_empty_response() {
        assert!(kbd_answers(&[], "hunter2").is_empty());
    }
}

#[cfg(test)]
mod ppk_tests {
    use super::{decode_private_key, is_ppk};

    /// PuTTY writes its files with CRLF line endings; joining here keeps the fixtures
    /// readable as plain line lists. Built from byte values so no escape survives a
    /// round trip through a code generator.
    fn ppk(lines: &[&str]) -> String {
        let crlf = String::from_utf8(vec![13, 10]).unwrap();
        lines.join(&crlf) + &crlf
    }

    // Real PuTTY-generated keys (test vectors from the ssh-key crate's own suite, which is
    // where our PPK support comes from). Passphrase for the encrypted one is "123".
    fn ed25519_plain() -> String {
        ppk(&[
            "PuTTY-User-Key-File-3: ssh-ed25519",
            "Encryption: none",
            "Comment: user@example.com",
            "Public-Lines: 2",
            "AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XF",
            "Sqti",
            "Private-Lines: 1",
            "AAAAILYGwiLRDBba4WxwpNRRc0cuxhfgXGVpINJuVsCPtZHt",
            "Private-MAC: 94140d0344fad6aa1bf7b71e9c93db11ccac8a232f8a51e11c024869d608c82d",
        ])
    }

    fn ed25519_encrypted() -> String {
        ppk(&[
            "PuTTY-User-Key-File-3: ssh-ed25519",
            "Encryption: aes256-cbc",
            "Comment: user@example.com",
            "Public-Lines: 2",
            "AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XF",
            "Sqti",
            "Key-Derivation: Argon2id",
            "Argon2-Memory: 8192",
            "Argon2-Passes: 34",
            "Argon2-Parallelism: 1",
            "Argon2-Salt: 63d1d43f7bf7700720496646a2f5ec17",
            "Private-Lines: 1",
            "DyWtExZ3dxFutnb12tIwXBC6kWdozrvP+r6faHKBGDb4+qEar9XBiC0BmGySMHUi",
            "Private-MAC: 52fd00d4ef47ebc506e4e709486c0c6bc0606e24fe2c6cb1b3d168f4da238a66",
        ])
    }

    #[test]
    fn recognises_a_ppk_by_content_not_extension() {
        assert!(is_ppk(&ed25519_plain()));
        assert!(is_ppk("  PuTTY-User-Key-File-2: ssh-rsa"));
        assert!(!is_ppk("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(!is_ppk(""));
    }

    /// The whole point: a .ppk goes in "Private key path" and works.
    #[test]
    fn an_unencrypted_ppk_loads() {
        let key = decode_private_key(&ed25519_plain(), None).expect("a PuTTY .ppk must load");
        assert_eq!(key.algorithm().as_str(), "ssh-ed25519");
    }

    /// The key-passphrase field feeds PPK decryption, so protected keys work too.
    #[test]
    fn an_encrypted_ppk_loads_with_its_passphrase() {
        let key = decode_private_key(&ed25519_encrypted(), Some("123"))
            .expect("an encrypted .ppk must load with the right passphrase");
        let plain = decode_private_key(&ed25519_plain(), None).unwrap();
        // Same identity either way — the encryption wraps the key, it does not change it.
        assert_eq!(
            key.public_key().to_openssh().unwrap(),
            plain.public_key().to_openssh().unwrap()
        );
    }

    /// A mistyped passphrase must not read as "this format is unsupported" — that is what
    /// made .ppk look unsupported when it was only mistyped.
    #[test]
    fn a_wrong_passphrase_names_the_format_and_the_passphrase() {
        let err = decode_private_key(&ed25519_encrypted(), Some("wrong"))
            .expect_err("a wrong passphrase must fail")
            .to_string();
        assert!(err.contains(".ppk"), "must name the format, got: {err}");
        assert!(err.contains("passphrase"), "must point at the passphrase, got: {err}");
    }

    /// A non-PPK failure keeps its own error instead of blaming PuTTY.
    #[test]
    fn a_broken_openssh_key_is_not_reported_as_a_putty_problem() {
        let err = decode_private_key("-----BEGIN OPENSSH PRIVATE KEY-----", None)
            .expect_err("garbage must fail")
            .to_string();
        assert!(!err.contains("PuTTYgen"), "got: {err}");
    }
}
