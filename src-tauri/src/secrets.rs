//! Secret storage backed by the OS keychain (Windows Credential Manager,
//! macOS Keychain, or the Linux Secret Service). Secrets (SSH passwords, key
//! passphrases, AI provider API keys, CLI tokens) are NEVER written to the
//! local SQLite database.
//!
//! A single key-based API serves every secret in the app. Callers pick the key
//! namespace: VPS secrets use the raw `vps_id`; AI provider keys use a prefixed
//! key like `ai:anthropic:key`. One code path, many jobs.

use anyhow::Result;
use base64::Engine;
use keyring::Entry;
use zeroize::Zeroizing;

const SERVICE: &str = "com.xconsole.app";

/// Keychain key holding the raw DB data key when "remember this device" is on. Stored as
/// base64 of the 32 random bytes — NOT the master password — so the keychain reveals
/// nothing about the password (DPAPI-protected on Windows, bound to the OS account).
pub const DATAKEY_KEY: &str = "db:datakey";

/// Remember the DB data key on this device (OS keychain).
pub fn set_data_key(key: &[u8; crate::crypto::KEY_LEN]) -> Result<()> {
    set_secret(DATAKEY_KEY, &base64::engine::general_purpose::STANDARD.encode(key))
}

/// Fetch the remembered DB data key, if any. Returns None if absent or malformed.
pub fn get_data_key() -> Result<Option<[u8; crate::crypto::KEY_LEN]>> {
    let Some(b64) = get_secret(DATAKEY_KEY)? else {
        return Ok(None);
    };
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| anyhow::anyhow!("decode data key: {e}"))?;
    if raw.len() != crate::crypto::KEY_LEN {
        return Ok(None);
    }
    let mut key = [0u8; crate::crypto::KEY_LEN];
    key.copy_from_slice(&raw);
    Ok(Some(key))
}

/// Forget the data key on this device (next launch will require the master password).
pub fn clear_data_key() -> Result<()> {
    delete_secret(DATAKEY_KEY)
}

fn entry(key: &str) -> Result<Entry> {
    // One credential per logical key.
    Ok(Entry::new(SERVICE, key)?)
}

/// Persist a secret under `key` into the OS keychain.
pub fn set_secret(key: &str, secret: &str) -> Result<()> {
    entry(key)?.set_password(secret)?;
    Ok(())
}

/// Fetch a secret by `key`. Wrapped in `Zeroizing` so the buffer is wiped on drop.
pub fn get_secret(key: &str) -> Result<Option<Zeroizing<String>>> {
    match entry(key)?.get_password() {
        Ok(s) => Ok(Some(Zeroizing::new(s))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Remove a secret by `key` from the keychain (best-effort).
pub fn delete_secret(key: &str) -> Result<()> {
    match entry(key)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Whether a secret exists for `key` (without copying it into a long-lived buffer).
pub fn has_secret(key: &str) -> bool {
    matches!(get_secret(key), Ok(Some(_)))
}

/// Keychain key namespace for an AI provider's API key / token.
pub fn provider_key(provider_id: &str) -> String {
    format!("ai:{provider_id}:key")
}

/// Keychain key for cloud account credentials (AWS keys, GCP JSON, TFC token).
pub fn cloud_account_key(account_id: &str) -> String {
    format!("cloud:{account_id}:secret")
}

/// Keychain key namespace for a VPS's app-managed SSH private key (PEM). Distinct
/// from the raw `vps_id` namespace (which holds a password / key-file passphrase),
/// so a managed key and a passphrase never collide. The private key lives only
/// here — never in SQLite or on disk.
pub fn ssh_key_key(vps_id: &str) -> String {
    format!("sshkey:{vps_id}")
}
