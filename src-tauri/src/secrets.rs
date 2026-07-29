//! Secret storage backed by the OS keychain (Windows Credential Manager,
//! macOS Keychain, or the Linux Secret Service). Secrets (SSH passwords, key
//! passphrases, AI provider API keys, CLI tokens) are NEVER written to the
//! local SQLite database.
//!
//! A single key-based API serves every secret in the app. Callers pick the key
//! namespace: VPS secrets use the raw `vps_id`; AI provider keys use a prefixed
//! key like `ai:anthropic:key`. One code path, many jobs.
//!
//! # Why the keychain alone is not enough
//!
//! The OS keychain protects secrets **at rest against another user or a stolen
//! drive** — on Windows the DPAPI master keys are wrapped with the login password.
//! It does NOT protect them against code running as the same user: any process the
//! user launches can call `CredRead` for this service name and read every credential
//! back in the clear. That makes "steal the credential store, use the credentials"
//! a one-liner for malware, an unattended script, or anyone at an unlocked desktop.
//!
//! So when the app lock is enabled, every secret is **encrypted with the database
//! data key before it reaches the keychain** (`set_wrapping_key`). The data key is
//! itself wrapped by the master password (see [`crate::crypto`]), so a stolen
//! keychain yields only ciphertext, and the master password is required to make any
//! of it usable. Values are tagged with [`WRAPPED_PREFIX`] so plaintext entries
//! written by older versions keep working and are re-wrapped in place on migration.
//!
//! The data key entry itself ([`DATAKEY_KEY`]) is necessarily stored raw — it is the
//! root of the chain and cannot encrypt itself. That entry only exists when the user
//! ticks "remember this device"; leaving it off keeps the master password mandatory.

use std::sync::{OnceLock, RwLock};

use anyhow::Result;
use base64::Engine;
use keyring::Entry;
use zeroize::Zeroizing;

use crate::crypto::{self, KEY_LEN};

const SERVICE: &str = "com.xconsole.app";

/// The key used to encrypt secrets before they go to the keychain. `None` when the
/// app lock is off (nothing to derive a key from) or while the app is still locked.
fn wrapping_slot() -> &'static RwLock<Option<[u8; KEY_LEN]>> {
    static SLOT: OnceLock<RwLock<Option<[u8; KEY_LEN]>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

/// Install (or clear) the key that secrets are encrypted with. Called on unlock, on
/// lock setup, and on teardown. A poisoned lock is recovered from rather than
/// panicking: failing here would strand the user at the unlock screen.
pub fn set_wrapping_key(key: Option<[u8; KEY_LEN]>) {
    let mut slot = wrapping_slot().write().unwrap_or_else(|e| e.into_inner());
    *slot = key;
}

/// Whether secrets are currently being encrypted before storage.
pub fn wrapping_active() -> bool {
    wrapping_slot()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

fn wrapping_key() -> Option<[u8; KEY_LEN]> {
    *wrapping_slot().read().unwrap_or_else(|e| e.into_inner())
}

/// Encrypt a secret for storage, when a wrapping key is available.
fn wrap_value(secret: &str) -> Result<String> {
    crypto::wrap_secret(wrapping_key().as_ref(), secret).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Decrypt a stored value. Plaintext (legacy) values pass through unchanged.
fn unwrap_value(stored: String) -> Result<Zeroizing<String>> {
    crypto::unwrap_secret(wrapping_key().as_ref(), &stored)
        .map(Zeroizing::new)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

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

/// Persist a secret under `key` into the OS keychain, encrypted with the data key
/// when the app lock is on.
pub fn set_secret(key: &str, secret: &str) -> Result<()> {
    // The data key is the root of the wrapping chain and can't encrypt itself.
    let stored = if key == DATAKEY_KEY {
        secret.to_string()
    } else {
        wrap_value(secret)?
    };
    entry(key)?.set_password(&stored)?;
    Ok(())
}

/// Fetch a secret by `key`, decrypting it if it was stored wrapped. Returned in
/// `Zeroizing` so the buffer is wiped on drop.
pub fn get_secret(key: &str) -> Result<Option<Zeroizing<String>>> {
    match entry(key)?.get_password() {
        Ok(s) if key == DATAKEY_KEY => Ok(Some(Zeroizing::new(s))),
        Ok(s) => Ok(Some(unwrap_value(s)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Move every secret in `keys` from the current wrapping key to `new_key`.
///
/// Used in both directions: enabling the lock converts plaintext entries to ciphertext
/// (`Some`), and disabling it converts them back (`None`) so they stay usable. Every
/// secret is read under the *old* key before the swap, because after it the previous
/// ciphertext would no longer be readable.
///
/// Best-effort per key — one unreadable entry must not abort the migration and strand
/// the rest. Returns the number of secrets rewritten.
pub fn rekey_all(keys: &[String], new_key: Option<[u8; KEY_LEN]>) -> usize {
    let mut pending = Vec::new();
    for key in keys {
        if key == DATAKEY_KEY {
            continue;
        }
        if let Ok(Some(secret)) = get_secret(key) {
            pending.push((key.clone(), secret));
        }
    }

    set_wrapping_key(new_key);

    let mut changed = 0;
    for (key, secret) in pending {
        if set_secret(&key, &secret).is_ok() {
            changed += 1;
        }
    }
    changed
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

/// Settings key recording that the user chose to encrypt stored credentials.
pub const ENCRYPT_SECRETS_SETTING: &str = "security.encrypt_secrets";

/// Has the user opted into encrypting keychain secrets?
///
/// Defaults to **off**. Turning it on is forward-only: builds that predate the feature
/// cannot read a wrapped secret and will fail every login with an authentication error,
/// so this must never flip on by itself.
pub fn encryption_opted_in(db: &crate::storage::Db) -> bool {
    matches!(db.get_setting(ENCRYPT_SECRETS_SETTING), Ok(Some(v)) if v == "true")
}

/// Every keychain key that holds a secret for the rows currently in the database.
///
/// The `keyring` crate offers no enumeration, so the set has to be reconstructed from
/// the records that own the secrets. Anything missed here simply stays in its current
/// form and is converted the next time it is written.
pub fn all_secret_keys(db: &crate::storage::Db) -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(list) = db.list_vps() {
        for v in list {
            keys.push(v.id.clone()); // password / key-file passphrase
            keys.push(ssh_key_key(&v.id)); // app-managed private key (PEM)
        }
    }
    if let Ok(list) = db.list_providers() {
        for p in list {
            keys.push(provider_key(&p.id));
        }
    }
    if let Ok(list) = db.list_cloud_accounts() {
        for a in list {
            keys.push(cloud_account_key(&a.id));
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The encryption itself is covered in `crypto`; what matters here is that the
    /// installed key is the one actually applied.
    #[test]
    fn wrapping_key_gates_encryption() {
        set_wrapping_key(None);
        assert!(!wrapping_active());
        assert_eq!(wrap_value("abc").unwrap(), "abc");

        let key = crypto::new_data_key();
        set_wrapping_key(Some(key));
        assert!(wrapping_active());
        let stored = wrap_value("abc").unwrap();
        assert!(stored.starts_with(crypto::WRAPPED_PREFIX));
        assert_eq!(&*unwrap_value(stored.clone()).unwrap(), "abc");

        // Clearing the key (locking) makes stored secrets unreadable again.
        set_wrapping_key(None);
        assert!(unwrap_value(stored).is_err());
    }
}
