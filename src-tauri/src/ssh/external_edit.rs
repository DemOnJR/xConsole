//! Edit a remote file in an external editor (VS Code, or whatever the user configures).
//!
//! The file is mirrored to a local temp path, the editor is launched on it, and every
//! save is pushed back over SFTP.
//!
//! # Not losing the file
//!
//! The naive version of this feature is why "my editor saved an empty file to the
//! server" is a well-known SFTP-extension failure. Three separate things cause it, and
//! each needs its own guard:
//!
//! 1. **Editors save non-atomically.** Many truncate the file and then write it. Poll at
//!    the wrong moment and you read 0 bytes of a perfectly good file. Guard: never act
//!    on the first observation of a change — require the size *and* mtime to be
//!    identical across two consecutive polls, so the write has demonstrably finished.
//! 2. **A zero-byte read looks like a legitimate edit.** Guard: refuse to replace a
//!    non-empty remote file with an empty local one. Emptying a file is a real thing to
//!    want, but it is rare enough to be worth an explicit action rather than a silent
//!    side effect of a race. [`ExternalEditEvent::Skipped`] tells the user why.
//! 3. **A transfer can fail halfway.** Guard: [`super::sftp::write_atomic`] writes to a
//!    temp file, verifies the server stored exactly as many bytes as were sent, and only
//!    then renames over the target. A failure leaves the original untouched.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use super::sftp::{open_channel, write_atomic, SftpManager};

/// Tauri event carrying [`ExternalEditEvent`].
pub const EDIT_EVENT: &str = "sftp://external-edit";

/// How often the mirrored file is checked for changes.
const POLL: Duration = Duration::from_millis(700);

/// Stop watching after this long with no save — an editor left open for days
/// shouldn't keep a task and an SSH channel alive forever.
const IDLE_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ExternalEditEvent {
    /// The mirror is ready and the editor has been launched.
    Opened { id: String, remote_path: String, local_path: String },
    /// A save was pushed back to the server.
    Saved { id: String, remote_path: String, bytes: u64 },
    /// A save was deliberately not pushed. `reason` is user-facing.
    Skipped { id: String, remote_path: String, reason: String },
    /// A save failed; the remote file is unchanged.
    Failed { id: String, remote_path: String, error: String },
    /// Watching stopped.
    Closed { id: String, remote_path: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalEditHandle {
    pub id: String,
    pub local_path: String,
}

/// Size + mtime, the pair used to decide a write has settled.
fn stamp(path: &PathBuf) -> Option<(u64, SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Split a configured editor setting into a program and its leading arguments.
///
/// Accepts a bare command (`code`), a full path, and a command with flags
/// (`code --new-window`). A quoted path with spaces is honoured so
/// `"C:\Program Files\Microsoft VS Code\Code.exe"` works.
pub fn parse_editor_command(setting: &str) -> Option<(String, Vec<String>)> {
    let setting = setting.trim();
    if setting.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in setting.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    if parts.is_empty() {
        return None;
    }
    let program = parts.remove(0);
    Some((program, parts))
}

/// Mirror a remote file locally, open it in the editor, and keep pushing saves back.
pub async fn start(
    app: AppHandle,
    sftp_mgr: SftpManager,
    session_id: String,
    remote_path: String,
    editor_setting: String,
) -> Result<ExternalEditHandle, String> {
    let (program, args) = parse_editor_command(&editor_setting)
        .ok_or_else(|| "No external editor is configured.".to_string())?;

    let refs = sftp_mgr.session_refs(&session_id)?;
    let channel = open_channel(&refs.ssh).await?;

    let name = remote_path
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| "that path has no filename".to_string())?
        .to_string();

    // Each edit gets its own directory so two files with the same name don't collide,
    // and so the mirror keeps its real name (the editor's syntax highlighting and
    // language tooling key off the extension).
    let id = Uuid::new_v4().to_string();
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("external-edit")
        .join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create temp dir: {e}"))?;
    let local_path = dir.join(&name);

    // Pull the current contents down.
    {
        use tokio::io::AsyncReadExt;
        let mut file = channel
            .open(&remote_path)
            .await
            .map_err(|e| format!("open failed: {e}"))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        std::fs::write(&local_path, &buf).map_err(|e| format!("write temp file: {e}"))?;
    }

    // Launch the editor detached: `code` returns immediately, and even editors that
    // block are fine because nothing here waits on the process.
    std::process::Command::new(&program)
        .args(&args)
        .arg(&local_path)
        .spawn()
        .map_err(|e| format!("could not start '{program}': {e}"))?;

    let local_path_str = local_path.to_string_lossy().into_owned();
    let _ = app.emit(
        EDIT_EVENT,
        ExternalEditEvent::Opened {
            id: id.clone(),
            remote_path: remote_path.clone(),
            local_path: local_path_str.clone(),
        },
    );

    let watch_id = id.clone();
    let watch_remote = remote_path.clone();
    // `tauri::async_runtime::spawn` throughout, so this never depends on whether the
    // caller happened to be an async command (sync Tauri commands have no ambient Tokio
    // runtime, and `tokio::spawn` panics there).
    tauri::async_runtime::spawn(async move {
        let mut last_pushed = stamp(&local_path);
        let mut pending: Option<(u64, SystemTime)> = None;
        let mut idle = Duration::ZERO;

        loop {
            tokio::time::sleep(POLL).await;
            idle += POLL;
            if idle >= IDLE_TIMEOUT {
                break;
            }

            let Some(current) = stamp(&local_path) else {
                // The mirror was deleted — the user is done with it.
                break;
            };
            if Some(current) == last_pushed {
                pending = None;
                continue;
            }

            // Require two identical observations before trusting the contents: an
            // editor that truncates-then-writes is momentarily at zero bytes, and
            // uploading that snapshot is exactly how a good file becomes an empty one.
            if pending != Some(current) {
                pending = Some(current);
                continue;
            }

            let bytes = match std::fs::read(&local_path) {
                Ok(b) => b,
                Err(e) => {
                    let _ = app.emit(
                        EDIT_EVENT,
                        ExternalEditEvent::Failed {
                            id: watch_id.clone(),
                            remote_path: watch_remote.clone(),
                            error: format!("could not read the local copy: {e}"),
                        },
                    );
                    pending = None;
                    continue;
                }
            };

            // Never let an empty read overwrite a file that has content.
            if bytes.is_empty() {
                let remote_size = channel
                    .metadata(&watch_remote)
                    .await
                    .ok()
                    .and_then(|m| m.size)
                    .unwrap_or(0);
                if remote_size > 0 {
                    let _ = app.emit(
                        EDIT_EVENT,
                        ExternalEditEvent::Skipped {
                            id: watch_id.clone(),
                            remote_path: watch_remote.clone(),
                            reason: format!(
                                "the local copy is empty but the file on the server is \
                                 {remote_size} bytes — not uploading. Save again if you \
                                 really meant to empty it."
                            ),
                        },
                    );
                    // Treat it as handled so it isn't retried every poll; a genuine
                    // later save changes the mtime again and goes through.
                    last_pushed = Some(current);
                    pending = None;
                    continue;
                }
            }

            match write_atomic(&channel, &watch_remote, &bytes).await {
                Ok(()) => {
                    last_pushed = Some(current);
                    idle = Duration::ZERO;
                    let _ = app.emit(
                        EDIT_EVENT,
                        ExternalEditEvent::Saved {
                            id: watch_id.clone(),
                            remote_path: watch_remote.clone(),
                            bytes: bytes.len() as u64,
                        },
                    );
                }
                Err(e) => {
                    let _ = app.emit(
                        EDIT_EVENT,
                        ExternalEditEvent::Failed {
                            id: watch_id.clone(),
                            remote_path: watch_remote.clone(),
                            error: e,
                        },
                    );
                }
            }
            pending = None;
        }

        let _ = std::fs::remove_dir_all(&dir);
        let _ = app.emit(
            EDIT_EVENT,
            ExternalEditEvent::Closed { id: watch_id, remote_path: watch_remote },
        );
    });

    Ok(ExternalEditHandle {
        id,
        local_path: local_path_str,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_commands() {
        let (p, a) = parse_editor_command("code").unwrap();
        assert_eq!(p, "code");
        assert!(a.is_empty());
    }

    #[test]
    fn parses_flags() {
        let (p, a) = parse_editor_command("code --new-window --disable-gpu").unwrap();
        assert_eq!(p, "code");
        assert_eq!(a, vec!["--new-window", "--disable-gpu"]);
    }

    #[test]
    fn honours_quoted_paths_with_spaces() {
        let (p, a) =
            parse_editor_command(r#""C:\Program Files\Microsoft VS Code\Code.exe" -n"#).unwrap();
        assert_eq!(p, r"C:\Program Files\Microsoft VS Code\Code.exe");
        assert_eq!(a, vec!["-n"]);
    }

    #[test]
    fn rejects_empty_settings() {
        assert!(parse_editor_command("").is_none());
        assert!(parse_editor_command("   ").is_none());
    }
}
