//! Files the agent creates on this PC (SSH key backups, downloads, writes).
//!
//! Private-key material is stored only as a file the user can copy. The agent
//! can list metadata (path, hash, size) but never read the bytes back.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// One recorded artifact (index row). `secret` means contents must never be
/// returned to the agent — only path/hash/size.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Artifact {
    pub id: String,
    pub name: String,
    pub path: String,
    /// "file" | "download" | "ssh_key" | "ssh_pub"
    pub kind: String,
    pub sha256: String,
    pub size: u64,
    pub secret: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub vps_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WrittenFile {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn looks_like_private_key_content(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(120)]).to_ascii_uppercase();
    head.contains("BEGIN OPENSSH PRIVATE KEY")
        || head.contains("BEGIN RSA PRIVATE KEY")
        || head.contains("BEGIN EC PRIVATE KEY")
        || head.contains("BEGIN PRIVATE KEY")
        || head.contains("BEGIN DSA PRIVATE KEY")
}

pub fn looks_like_secret_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let name = Path::new(&p)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".pub") {
        return false;
    }
    if name == "id_rsa"
        || name == "id_ed25519"
        || name == "id_ecdsa"
        || name == "id_dsa"
        || name.ends_with("_rsa")
        || name.ends_with("_ed25519")
    {
        return true;
    }
    if p.contains("/artifacts/ssh/") && !name.ends_with(".pub") {
        return true;
    }
    name == "xconsole.db" || name.ends_with(".db-wal") || name.ends_with(".db-shm")
}

/// Write bytes, then re-read and compare SHA-256 so a half-written file is rejected.
pub fn write_verified(path: &Path, bytes: &[u8]) -> Result<WrittenFile, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
        }
    }
    write_restricted(path, bytes)?;
    let read_back = fs::read(path).map_err(|e| format!("verify read failed: {e}"))?;
    let expected = sha256_hex(bytes);
    let got = sha256_hex(&read_back);
    if expected != got || read_back.len() != bytes.len() {
        let _ = fs::remove_file(path);
        return Err(format!(
            "integrity check failed for {}: wrote {} bytes sha256 {expected}, read {} bytes sha256 {got}",
            path.display(),
            bytes.len(),
            read_back.len()
        ));
    }
    Ok(WrittenFile {
        path: path.to_path_buf(),
        sha256: expected,
        size: bytes.len() as u64,
    })
}

fn write_restricted(path: &Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("write failed: {e}"))?;
        f.write_all(bytes).map_err(|e| format!("write failed: {e}"))?;
        f.sync_all().map_err(|e| format!("fsync failed: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes).map_err(|e| format!("write failed: {e}"))?;
    }
    Ok(())
}

pub fn artifacts_root(app_data: &Path) -> PathBuf {
    app_data.join("artifacts")
}

pub fn ssh_backup_dir(app_data: &Path, vps_id: &str, vps_name: &str) -> PathBuf {
    let slug: String = vps_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let short: String = vps_id.chars().take(8).collect();
    let folder = if slug.is_empty() {
        format!("ssh-{short}")
    } else {
        format!("{slug}-{short}")
    };
    artifacts_root(app_data).join("ssh").join(folder)
}

pub fn verify_file(path: &Path, expected_sha: &str) -> Result<bool, String> {
    let bytes = fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    Ok(sha256_hex(&bytes) == expected_sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_verified_round_trips() {
        let dir = std::env::temp_dir().join("xconsole_artifact_test");
        let path = dir.join("ok.txt");
        let written = write_verified(&path, b"hello-key").expect("write");
        assert_eq!(written.size, 9);
        assert_eq!(written.sha256, sha256_hex(b"hello-key"));
        assert!(verify_file(&path, &written.sha256).unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_private_key_content_and_paths() {
        assert!(looks_like_private_key_content(
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjE"
        ));
        assert!(!looks_like_private_key_content(b"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5"));
        assert!(looks_like_secret_path(r"C:\Users\me\.ssh\id_ed25519"));
        assert!(!looks_like_secret_path(r"C:\Users\me\.ssh\id_ed25519.pub"));
        assert!(looks_like_secret_path(
            r"C:\Users\me\AppData\Roaming\com.xconsole.app\artifacts\ssh\box-abc\id_ed25519"
        ));
    }
}
