//! `db.lock.json` — the small UNENCRYPTED bootstrap manifest that lets the app
//! reconstruct the DB data key from the master password.
//!
//! It cannot live inside the encrypted DB (chicken-and-egg: you'd need the data key to
//! read the thing that tells you how to derive the data key). It holds only a random
//! salt, the password-wrapped data key (AES-256-GCM — useless without the password), the
//! KDF iteration count, the lock flag, and a persist generation counter (for crash
//! recovery). A stolen `db.lock.json` + `xconsole.db.enc` pair is worthless without the
//! master password (or the keychain copy).

use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

pub const MANIFEST: &str = "db.lock.json";

/// Lowest PBKDF2 iteration count we will accept from a manifest at unlock time. The manifest is
/// plaintext and unauthenticated, so we refuse a suspiciously-low (corrupt or hand-edited) count
/// rather than silently deriving a weak key. Every manifest we write uses
/// [`crate::crypto::DEFAULT_ITERS`] (600k), so this never rejects a legitimate file.
pub const MIN_KDF_ITERS: u32 = 100_000;

#[derive(Serialize, Deserialize, Clone)]
pub struct LockManifest {
    pub version: u32,
    /// base64 of the 16-byte salt.
    pub salt: String,
    /// base64 of the wrapped data key (nonce||ciphertext||tag).
    pub wrapped_key: String,
    pub kdf_iters: u32,
    pub lock_enabled: bool,
    /// Bumped on every successful persist of `xconsole.db.enc`; used so crash recovery
    /// can decide whether a leftover plaintext working file is actually newer than the
    /// encrypted artifact (never infer recency from file presence alone).
    #[serde(default)]
    pub generation: u64,
}

impl LockManifest {
    pub fn salt_bytes(&self) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.salt)
            .unwrap_or_default()
    }
    pub fn wrapped_bytes(&self) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.wrapped_key)
            .unwrap_or_default()
    }
}

pub fn manifest_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MANIFEST)
}

pub fn read(data_dir: &Path) -> Option<LockManifest> {
    let s = std::fs::read_to_string(manifest_path(data_dir)).ok()?;
    serde_json::from_str(&s).ok()
}

/// Write the manifest atomically (temp + rename) so a crash can't leave it half-written.
pub fn write(data_dir: &Path, m: &LockManifest) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(m)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let path = manifest_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)
}

/// Whether the DB lock is configured + enabled for this install.
pub fn is_lock_enabled(data_dir: &Path) -> bool {
    read(data_dir).map(|m| m.lock_enabled).unwrap_or(false)
}

/// Build a manifest that wraps `data_key` under `password` (used by setup / change-password).
pub fn build_manifest(
    password: &str,
    data_key: &[u8; crate::crypto::KEY_LEN],
    generation: u64,
) -> Result<LockManifest, String> {
    let salt = crate::crypto::new_salt();
    let iters = crate::crypto::DEFAULT_ITERS;
    let wrapped = crate::crypto::wrap_data_key(password, &salt, iters, data_key)?;
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(LockManifest {
        version: 1,
        salt: b64.encode(&salt),
        wrapped_key: b64.encode(&wrapped),
        kdf_iters: iters,
        lock_enabled: true,
        generation,
    })
}

/// Recover the data key from a manifest + master password. `Err` on a wrong password.
pub fn unlock(m: &LockManifest, password: &str) -> Result<[u8; crate::crypto::KEY_LEN], String> {
    if m.kdf_iters < MIN_KDF_ITERS {
        return Err("unsupported lock file (iteration count too low) — refusing to unlock".into());
    }
    crate::crypto::unwrap_data_key(password, &m.salt_bytes(), m.kdf_iters, &m.wrapped_bytes())
}
