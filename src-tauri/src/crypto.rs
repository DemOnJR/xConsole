//! At-rest encryption primitives, built on `ring` (already in the dependency tree via
//! russh/rustls — no OpenSSL, so the clone+compile build stays clean on MinGW).
//!
//! Design (key-wrapping):
//! - A random 256-bit **data key** encrypts the database (AES-256-GCM).
//! - The data key is **wrapped** (encrypted) by a key derived from the user's master
//!   password (PBKDF2-HMAC-SHA256). `wrap`/`unwrap` are just `encrypt`/`decrypt` of the
//!   data key — and because GCM authenticates, a wrong password makes `unwrap` FAIL,
//!   so the wrapped blob doubles as the password verifier.
//! - "Remember on this device" stores the data key in the OS keychain (DPAPI), so the
//!   app unlocks without a prompt while the DB file itself stays encrypted at rest.
//!
//! There is no recovery path by design: lose the password (and the device key) and the
//! data is unrecoverable — which is exactly what makes a stolen `.db` file useless.

use std::num::NonZeroU32;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};

pub const KEY_LEN: usize = 32; // AES-256
pub const SALT_LEN: usize = 16;
/// Default PBKDF2 iterations. High enough to make password guessing expensive. Stored
/// in the lock manifest (not hardcoded at unlock) so it can be raised later without
/// locking out existing users — see [`derive_key_iters`].
pub const DEFAULT_ITERS: u32 = 600_000;
const TAG_LEN: usize = 16; // AES-GCM tag

/// Fill `n` cryptographically-random bytes.
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    SystemRandom::new()
        .fill(&mut buf)
        .expect("system RNG unavailable");
    buf
}

/// A fresh random salt for password derivation.
pub fn new_salt() -> Vec<u8> {
    random_bytes(SALT_LEN)
}

/// A fresh random 256-bit data key (the key that actually encrypts the DB).
pub fn new_data_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    SystemRandom::new().fill(&mut k).expect("system RNG unavailable");
    k
}

/// Derive a 256-bit key from a master password + salt with an explicit iteration count
/// (PBKDF2-HMAC-SHA256). The lock manifest stores the iteration count it was created with,
/// so unlocking always uses the right one even if the default is bumped later.
pub fn derive_key_iters(password: &str, salt: &[u8], iters: u32) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iters.max(1)).expect("nonzero iters"),
        salt,
        password.as_bytes(),
        &mut key,
    );
    key
}

/// Derive a 256-bit key using the current default iteration count.
pub fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    derive_key_iters(password, salt, DEFAULT_ITERS)
}

/// AES-256-GCM encrypt. Output layout: `nonce(12) || ciphertext || tag(16)`.
pub fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let unbound = UnboundKey::new(&AES_256_GCM, key).map_err(|_| "invalid key".to_string())?;
    let sealing = LessSafeKey::new(unbound);

    let mut nonce_bytes = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| "rng failed".to_string())?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "encryption failed".to_string())?;

    let mut out = Vec::with_capacity(NONCE_LEN + in_out.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&in_out);
    Ok(out)
}

/// AES-256-GCM decrypt of an [`encrypt`] output. Fails (authentication error) if the
/// key is wrong or the data was tampered with — this is what verifies the password.
pub fn decrypt(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < NONCE_LEN + TAG_LEN {
        return Err("ciphertext too short".into());
    }
    let unbound = UnboundKey::new(&AES_256_GCM, key).map_err(|_| "invalid key".to_string())?;
    let opening = LessSafeKey::new(unbound);

    let (nonce_bytes, ct) = data.split_at(NONCE_LEN);
    let nonce =
        Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| "invalid nonce".to_string())?;

    let mut in_out = ct.to_vec();
    let plaintext = opening
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "decryption failed — wrong password or corrupted data".to_string())?;
    Ok(plaintext.to_vec())
}

/// Wrap (encrypt) the data key with the password-derived key. The result is stored on
/// disk; unwrapping it with the wrong password fails, so it is also the verifier.
pub fn wrap_data_key(
    password: &str,
    salt: &[u8],
    iters: u32,
    data_key: &[u8; KEY_LEN],
) -> Result<Vec<u8>, String> {
    let kek = derive_key_iters(password, salt, iters);
    encrypt(&kek, data_key)
}

/// Unwrap (decrypt) the data key. Returns `Err` on a wrong password.
pub fn unwrap_data_key(
    password: &str,
    salt: &[u8],
    iters: u32,
    wrapped: &[u8],
) -> Result<[u8; KEY_LEN], String> {
    let kek = derive_key_iters(password, salt, iters);
    let raw = decrypt(&kek, wrapped)?;
    if raw.len() != KEY_LEN {
        return Err("unwrapped key has the wrong length".into());
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&raw);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wrong_key() {
        let key = new_data_key();
        let ct = encrypt(&key, b"chats + workspaces").unwrap();
        assert_eq!(decrypt(&key, &ct).unwrap(), b"chats + workspaces");
        // A different key must fail (authentication).
        assert!(decrypt(&new_data_key(), &ct).is_err());
    }

    #[test]
    fn password_wrapping_verifies() {
        let salt = new_salt();
        let data_key = new_data_key();
        let it = DEFAULT_ITERS;
        let wrapped = wrap_data_key("correct horse", &salt, it, &data_key).unwrap();
        assert_eq!(unwrap_data_key("correct horse", &salt, it, &wrapped).unwrap(), data_key);
        // Wrong password fails to unwrap.
        assert!(unwrap_data_key("wrong password", &salt, it, &wrapped).is_err());
        // Different iteration counts derive different keys (so kdf_iters matters).
        assert_ne!(derive_key_iters("p", &salt, 1000), derive_key_iters("p", &salt, 2000));
    }
}
