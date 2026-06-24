//! SSH keypair generation and the "switch a server from passwords to key auth"
//! workflow. Keys are generated in-process (no shelling out to `ssh-keygen`,
//! which isn't guaranteed on Windows) and the private key never touches disk or
//! SQLite — it lives only in the OS keychain (see [`crate::secrets`]).

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey};
use zeroize::Zeroizing;

use crate::secrets;
use crate::storage::models::{AuthType, VpsInput};
use crate::storage::Db;

use super::manager::SessionManager;
use super::shell_quote;

/// A freshly generated Ed25519 keypair. `private_pem` is OpenSSH PEM and is wiped
/// from memory on drop; `public_openssh` is the single-line authorized_keys form.
pub struct GeneratedKey {
    pub private_pem: Zeroizing<String>,
    pub public_openssh: String,
    pub fingerprint: String,
}

/// Generate a new Ed25519 SSH keypair using the OS CSPRNG.
pub fn generate_ed25519() -> Result<GeneratedKey, String> {
    let mut rng = rand_core::OsRng;
    let key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|e| format!("key generation failed: {e}"))?;
    let private_pem = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| format!("encoding private key failed: {e}"))?;
    let public = key.public_key();
    let public_openssh = public
        .to_openssh()
        .map_err(|e| format!("encoding public key failed: {e}"))?;
    let fingerprint = public.fingerprint(HashAlg::Sha256).to_string();
    Ok(GeneratedKey {
        private_pem,
        public_openssh,
        fingerprint,
    })
}

/// Result of [`setup_key_auth`], returned to the agent/UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupReport {
    pub vps_id: String,
    pub fingerprint: String,
    pub public_openssh: String,
}

/// Switch a VPS from its current auth to an app-managed SSH key:
/// generate a keypair, install the public key on the server over the *current*
/// connection, store the private key in the keychain, flip the record to key
/// auth, and verify a fresh key-based connection works (rolling back on failure).
/// Password login on the server is left untouched (no lockout risk).
pub async fn setup_key_auth(
    db: &Db,
    sessions: &SessionManager,
    vps_id: &str,
) -> Result<SetupReport, String> {
    let vps = db
        .get_vps(vps_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VPS not found".to_string())?;

    // 1. Confirm we can reach the server with the CURRENT credentials first.
    let probe = sessions
        .run_command(vps_id, "true")
        .await
        .map_err(|e| format!("cannot reach server with current credentials: {e}"))?;
    if probe.exit_code != 0 {
        return Err("cannot reach server with current credentials".into());
    }

    // 2. Generate the keypair.
    let key = generate_ed25519()?;

    // 3. Install the public key into ~/.ssh/authorized_keys (idempotent).
    let pk = shell_quote(&key.public_openssh);
    let install = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && \
         (grep -qxF {pk} ~/.ssh/authorized_keys || printf '%s\\n' {pk} >> ~/.ssh/authorized_keys) && \
         chmod 600 ~/.ssh/authorized_keys"
    );
    let out = sessions
        .run_command(vps_id, &install)
        .await
        .map_err(|e| format!("installing public key failed: {e}"))?;
    if out.exit_code != 0 {
        return Err(format!(
            "installing public key failed: {}",
            out.stderr.trim()
        ));
    }

    // 4. Store the private key in the OS keychain (encrypted at rest, never on disk).
    secrets::set_secret(&secrets::ssh_key_key(vps_id), &key.private_pem)
        .map_err(|e| format!("storing private key failed: {e}"))?;

    // 5. Flip the record to key auth. key_path = None → auth resolves to the managed key.
    let to_key = vps_input_from(&vps, AuthType::Key, None);
    db.upsert_vps(&to_key).map_err(|e| e.to_string())?;

    // 6. Verify a fresh connection (re-resolves auth from the updated row → managed key).
    let verify = sessions.run_command(vps_id, "true").await;
    let verified = matches!(&verify, Ok(o) if o.exit_code == 0);
    if !verified {
        // Roll back so the user is never stranded.
        let rollback = vps_input_from(&vps, vps.auth_type.clone(), vps.key_path.clone());
        let _ = db.upsert_vps(&rollback);
        let _ = secrets::delete_secret(&secrets::ssh_key_key(vps_id));
        let detail = match verify {
            Err(e) => e,
            Ok(o) => o.stderr.trim().to_string(),
        };
        return Err(format!("key auth verification failed, rolled back: {detail}"));
    }

    Ok(SetupReport {
        vps_id: vps.id,
        fingerprint: key.fingerprint,
        public_openssh: key.public_openssh,
    })
}

/// Build a `VpsInput` from an existing row, overriding only the auth fields.
/// `secret: None` means the existing password/passphrase secret is left as-is.
fn vps_input_from(
    vps: &crate::storage::models::Vps,
    auth_type: AuthType,
    key_path: Option<String>,
) -> VpsInput {
    VpsInput {
        id: Some(vps.id.clone()),
        name: vps.name.clone(),
        host: vps.host.clone(),
        port: vps.port,
        username: vps.username.clone(),
        auth_type,
        key_path,
        tags: vps.tags.clone(),
        secret: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::decode_secret_key;

    #[test]
    fn generated_key_round_trips_and_is_ed25519() {
        let key = generate_ed25519().expect("generate");
        assert!(key.public_openssh.starts_with("ssh-ed25519 "));
        assert!(key.fingerprint.starts_with("SHA256:"));
        // The PEM must decode back to a usable Ed25519 private key.
        let decoded = decode_secret_key(&key.private_pem, None).expect("decode");
        assert_eq!(decoded.algorithm(), Algorithm::Ed25519);
    }
}
