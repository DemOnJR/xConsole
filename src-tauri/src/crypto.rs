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

/// Marks a stored secret as ciphertext rather than a raw value. Values without this
/// prefix are legacy plaintext, written before secret wrapping existed.
pub const WRAPPED_PREFIX: &str = "xcw1:";

/// Encode a secret for storage in the OS keychain: `xcw1:` + base64 of the AES-GCM
/// blob. With no key (app lock off) the secret is returned unchanged, because there is
/// nothing to derive an encryption key from.
///
/// Kept here rather than in `secrets` so it stays free of the keychain dependency and
/// can be tested on its own.
pub fn wrap_secret(key: Option<&[u8; KEY_LEN]>, secret: &str) -> Result<String, String> {
    let Some(key) = key else {
        return Ok(secret.to_string());
    };
    let ct = encrypt(key, secret.as_bytes())?;
    Ok(format!(
        "{WRAPPED_PREFIX}{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ct)
    ))
}

/// Decode a value produced by [`wrap_secret`]. Untagged values pass through as-is, so
/// enabling the lock never orphans secrets saved by an older build.
pub fn unwrap_secret(key: Option<&[u8; KEY_LEN]>, stored: &str) -> Result<String, String> {
    let Some(b64) = stored.strip_prefix(WRAPPED_PREFIX) else {
        return Ok(stored.to_string());
    };
    let key = key.ok_or_else(|| {
        "this secret is encrypted — unlock xConsole with your master password first".to_string()
    })?;
    let ct = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
        .map_err(|e| format!("decode secret: {e}"))?;
    let raw = decrypt(key, &ct)?;
    String::from_utf8(raw).map_err(|_| "secret is not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_wrapping_round_trips_and_hides_the_value() {
        let key = new_data_key();
        let stored = wrap_secret(Some(&key), "hunter2").unwrap();
        assert!(stored.starts_with(WRAPPED_PREFIX), "value must be tagged");
        assert!(!stored.contains("hunter2"), "plaintext must not survive");
        assert_eq!(unwrap_secret(Some(&key), &stored).unwrap(), "hunter2");
    }

    #[test]
    fn wrapped_secret_is_useless_without_the_right_key() {
        let key = new_data_key();
        let stored = wrap_secret(Some(&key), "id_rsa-contents").unwrap();
        // Copied the credential store but has no data key.
        assert!(unwrap_secret(None, &stored).is_err());
        // Has a data key, but from a different master password.
        assert!(unwrap_secret(Some(&new_data_key()), &stored).is_err());
    }

    #[test]
    fn legacy_plaintext_values_still_read_either_way() {
        assert_eq!(unwrap_secret(None, "plain-token").unwrap(), "plain-token");
        let key = new_data_key();
        assert_eq!(unwrap_secret(Some(&key), "plain-token").unwrap(), "plain-token");
    }

    #[test]
    fn no_key_means_no_encryption() {
        assert_eq!(wrap_secret(None, "abc").unwrap(), "abc");
    }

    #[test]
    fn identical_secrets_encrypt_differently() {
        let key = new_data_key();
        let a = wrap_secret(Some(&key), "same").unwrap();
        let b = wrap_secret(Some(&key), "same").unwrap();
        assert_ne!(a, b, "a fresh nonce per write must prevent ciphertext equality");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let key = new_data_key();
        let stored = wrap_secret(Some(&key), "root-password").unwrap();
        // Flip a byte in the base64 body; GCM authentication must catch it.
        let mut bytes: Vec<u8> = stored.into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(unwrap_secret(Some(&key), &tampered).is_err());
    }

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
