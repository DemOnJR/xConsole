//! Dropping local files (and pasted screenshots) onto a terminal.
//!
//! The terminal has only a shell session, so the bytes go up over SFTP — reusing the file
//! browser's connection when there is one — and the caller then types the resulting remote
//! path into the shell. Keeping the upload here rather than in the frontend means the
//! webview never needs filesystem access: it passes the paths the OS handed it, and
//! nothing else.

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ssh::SftpManager;

/// Cap for the inline preview returned to the UI. Big enough for a screenshot, small
/// enough that a dropped disk image does not travel back across the IPC bridge as base64.
const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

/// Bytes supplied directly by the frontend (a pasted screenshot has no path on disk).
#[derive(Deserialize)]
pub struct InlineFile {
    pub name: String,
    pub content_b64: String,
}

#[derive(Serialize)]
pub struct Uploaded {
    pub name: String,
    /// Full remote path, ready to be shell-quoted and typed.
    pub path: String,
    pub size: u64,
    /// Base64 of the file itself when it is a small image, for a thumbnail. Not a
    /// generated thumbnail — there is no image decoder in the tree, and the browser
    /// already scales an <img> perfectly well.
    pub preview_b64: Option<String>,
    pub is_image: bool,
}

/// Reduce whatever the OS gave us to a single safe filename component.
///
/// The name reaches two dangerous places: a remote path, and a shell command line. This
/// handles the first — `../../.ssh/authorized_keys` is a legal thing to name a file, and
/// dropping it must not write there. The second is the caller's job (shell-quoting).
fn safe_name(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches('.');
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .collect();
    if cleaned.is_empty() {
        "dropped-file".to_string()
    } else {
        cleaned
    }
}

fn is_image_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"]
        .iter()
        .any(|e| l.ends_with(e))
}

fn join_remote(dir: &str, name: &str) -> String {
    let d = dir.trim_end_matches('/');
    if d.is_empty() {
        format!("/{name}")
    } else {
        format!("{d}/{name}")
    }
}

/// Pick a name that is not already taken, so a second screenshot never silently replaces
/// the first. `shot.png` → `shot-1.png` → `shot-2.png`.
async fn free_name(
    sftp: &SftpManager,
    session_id: &str,
    dir: &str,
    name: &str,
) -> String {
    let taken = |candidate: &str| -> String { join_remote(dir, candidate) };
    if sftp.stat_missing(session_id, &taken(name)).await {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1..1000 {
        let candidate = format!("{stem}-{n}{ext}");
        if sftp.stat_missing(session_id, &taken(&candidate)).await {
            return candidate;
        }
    }
    format!("{stem}-{}{ext}", uuid::Uuid::new_v4())
}

/// Upload dropped/pasted files to `dir` on `vps_id` and report where they landed.
#[tauri::command]
pub async fn terminal_upload(
    sftp: State<'_, SftpManager>,
    vps_id: String,
    dir: String,
    local_paths: Vec<String>,
    inline: Vec<InlineFile>,
) -> Result<Vec<Uploaded>, String> {
    let session_id = sftp.session_for_vps(&vps_id).await?;
    let mut out = Vec::new();

    for path in local_paths {
        let meta = std::fs::metadata(&path).map_err(|e| format!("{path}: {e}"))?;
        if meta.is_dir() {
            // Directories need the recursive transfer engine and a progress UI; silently
            // uploading one file out of a dropped folder would be worse than saying no.
            return Err(format!(
                "{} is a folder — drop it on the file browser instead, which can show progress.",
                safe_name(&path)
            ));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
        let name = safe_name(&path);
        out.push(put(&sftp, &session_id, &dir, &name, &bytes).await?);
    }

    for f in inline {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(f.content_b64.as_bytes())
            .map_err(|e| format!("invalid base64: {e}"))?;
        let name = safe_name(&f.name);
        out.push(put(&sftp, &session_id, &dir, &name, &bytes).await?);
    }

    Ok(out)
}

async fn put(
    sftp: &SftpManager,
    session_id: &str,
    dir: &str,
    name: &str,
    bytes: &[u8],
) -> Result<Uploaded, String> {
    let name = free_name(sftp, session_id, dir, name).await;
    let remote = join_remote(dir, &name);
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    sftp.write(session_id, &remote, &b64).await?;

    let is_image = is_image_name(&name);
    let preview_b64 = if is_image && bytes.len() as u64 <= MAX_PREVIEW_BYTES {
        Some(b64)
    } else {
        None
    };
    Ok(Uploaded {
        name,
        path: remote,
        size: bytes.len() as u64,
        preview_b64,
        is_image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A filename is attacker-adjacent input: it comes from a file someone else may have
    /// created, and it is about to become a remote path.
    #[test]
    fn safe_name_reduces_to_one_component() {
        assert_eq!(safe_name("../../.ssh/authorized_keys"), "authorized_keys");
        assert_eq!(safe_name(r"C:\Users\me\shot.png"), "shot.png");
        assert_eq!(safe_name("/etc/passwd"), "passwd");
        assert_eq!(safe_name(".."), "dropped-file");
        assert_eq!(safe_name(""), "dropped-file");
        assert_eq!(safe_name("ok\u{0}name.txt"), "okname.txt");
    }

    #[test]
    fn join_remote_never_doubles_or_drops_a_slash() {
        assert_eq!(join_remote("/tmp", "a.txt"), "/tmp/a.txt");
        assert_eq!(join_remote("/tmp/", "a.txt"), "/tmp/a.txt");
        assert_eq!(join_remote("", "a.txt"), "/a.txt");
        assert_eq!(join_remote("/", "a.txt"), "/a.txt");
    }

    #[test]
    fn images_are_recognised_case_insensitively() {
        assert!(is_image_name("Screenshot.PNG"));
        assert!(is_image_name("a.jpeg"));
        assert!(!is_image_name("notes.md"));
        assert!(!is_image_name("png"));
    }
}
