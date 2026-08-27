//! In-app updater for the clone+compile distribution.
//!
//! The app is installed by cloning + compiling from GitHub (see `installer/`). To
//! update, we compare the local checkout's HEAD against the **active channel branch**
//! (`main` or `dev`) on GitHub and, on the user's accept, re-run the installer — which
//! does `git fetch + reset --hard` for that branch, rebuilds, and swaps in the new exe.
//!
//! Channel is stored in:
//!   * SQLite setting `update.channel` (app UI)
//!   * `%LOCALAPPDATA%\xConsole\channel` (read by the installer / next launch)
//!
//! USER DATA IS SAFE: it lives in the app-data dir (`%APPDATA%\com.xconsole.app`) and the
//! OS keychain — never in the install tree. We also snapshot DB + agent files before update.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::storage::Db;

const REPO: &str = "DemOnJR/xConsole";
const KEEP_BACKUPS: usize = 5;
const CHANNEL_SETTING: &str = "update.channel";

/// Allowed channels (git branches).
pub fn is_valid_channel(s: &str) -> bool {
    matches!(s, "main" | "dev")
}

/// Where the installer placed the app (and itself). Mirrors the installer's `base_dir`.
fn install_base() -> PathBuf {
    if let Ok(b) = std::env::var("XCONSOLE_INSTALL_BASE") {
        if !b.trim().is_empty() {
            return PathBuf::from(b);
        }
    }
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("xConsole")
}

fn channel_file() -> PathBuf {
    install_base().join("channel")
}

/// Resolve the active update channel: setting → channel file → `main`.
pub fn active_channel(db: &Db) -> String {
    if let Ok(Some(s)) = db.get_setting(CHANNEL_SETTING) {
        let s = s.trim().to_string();
        if is_valid_channel(&s) {
            return s;
        }
    }
    if let Ok(s) = std::fs::read_to_string(channel_file()) {
        let s = s.trim().to_string();
        if is_valid_channel(&s) {
            return s;
        }
    }
    "main".into()
}

fn write_channel_file(channel: &str) {
    let base = install_base();
    let _ = std::fs::create_dir_all(&base);
    let _ = std::fs::write(channel_file(), format!("{channel}\n"));
}

/// Persist channel to SQLite + the installer's channel file.
pub fn set_channel(db: &Db, channel: &str) -> Result<(), String> {
    if !is_valid_channel(channel) {
        return Err(format!("invalid channel '{channel}' (use main or dev)"));
    }
    db.set_setting(CHANNEL_SETTING, channel)
        .map_err(|e| e.to_string())?;
    write_channel_file(channel);
    // Retarget the local source now so the selected channel is visible and authoritative
    // before the next update check. A failed switch must not be reported as success.
    let src = install_base().join("src");
    if src.join(".git").exists() {
        ensure_checkout_branch(&src, channel)
            .map_err(|e| format!("could not switch source checkout to '{channel}': {e}"))?;
    }
    Ok(())
}

/// Run a git command in `cwd`. Uses PATH (system/Hermes git is fine).
fn git_in(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git not found: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!(
            "git {} failed: {}{}",
            args.first().unwrap_or(&""),
            err.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!(" ({})", stdout.trim())
            }
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Point the local source tree at `channel` without a full rebuild.
///
/// Handles shallow single-branch clones of `main` that never had `origin/dev` by
/// rewriting `remote.origin.fetch` and fetching an explicit refspec.
fn ensure_checkout_branch(src: &Path, channel: &str) -> Result<(), String> {
    if !is_valid_channel(channel) {
        return Err(format!("invalid channel '{channel}'"));
    }
    if !src.join(".git").exists() {
        return Err("no local source checkout".into());
    }
    // Already there?
    if local_branch(src).as_deref() == Some(channel) {
        write_channel_file(channel);
        return Ok(());
    }
    let _ = git_in(
        src,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    let refspec = format!("+refs/heads/{channel}:refs/remotes/origin/{channel}");
    git_in(src, &["fetch", "--depth", "1", "origin", &refspec])
        .or_else(|_| git_in(src, &["fetch", "--depth", "1", "origin", channel]))?;
    let remote = format!("origin/{channel}");
    git_in(src, &["checkout", "-B", channel, &remote])
        .or_else(|_| git_in(src, &["checkout", "-B", channel, "FETCH_HEAD"]))?;
    let _ = git_in(src, &["reset", "--hard", "HEAD"]);
    let _ = git_in(src, &["branch", "--set-upstream-to", &remote, channel]);
    write_channel_file(channel);
    Ok(())
}

/// The SHA the local source is checked out at (any branch).
fn local_head(src: &Path) -> Option<String> {
    let git = src.join(".git");
    // Prefer symbolic HEAD → branch ref (works for main and dev).
    if let Ok(head) = std::fs::read_to_string(git.join("HEAD")) {
        let head = head.trim();
        if let Some(r) = head.strip_prefix("ref: ") {
            let ref_path = git.join(r.trim());
            if let Ok(s) = std::fs::read_to_string(&ref_path) {
                let s = s.trim();
                if s.len() >= 7 {
                    return Some(s.to_string());
                }
            }
            // packed-refs fallback for that ref
            if let Ok(packed) = std::fs::read_to_string(git.join("packed-refs")) {
                let needle = r.trim();
                for line in packed.lines() {
                    if line.trim_end().ends_with(needle) {
                        if let Some(sha) = line.split_whitespace().next() {
                            return Some(sha.to_string());
                        }
                    }
                }
            }
        } else if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) {
            // Detached HEAD — raw SHA.
            return Some(head.to_string());
        }
    }
    // Legacy: main branch only (older installs).
    if let Ok(s) = std::fs::read_to_string(git.join("refs").join("heads").join("main")) {
        let s = s.trim();
        if s.len() >= 7 {
            return Some(s.to_string());
        }
    }
    None
}

/// Best-effort: which branch the checkout is currently on.
fn local_branch(src: &Path) -> Option<String> {
    let head = std::fs::read_to_string(src.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(r) = head.strip_prefix("ref: refs/heads/") {
        let b = r.trim();
        if is_valid_channel(b) {
            return Some(b.to_string());
        }
        return Some(b.to_string());
    }
    None
}

fn update_decision(
    local_sha: Option<&str>,
    local_branch: Option<&str>,
    channel: &str,
    latest_sha: &str,
) -> (bool, Option<String>) {
    let short = |s: &str| s.chars().take(7).collect::<String>();
    let branch_mismatch = local_branch.map(|b| b != channel).unwrap_or(true);
    match local_sha {
        Some(local) if !latest_sha.is_empty() => {
            let commit_changed = short(local) != short(latest_sha);
            let available = commit_changed || branch_mismatch;
            let note = if available && branch_mismatch {
                Some(format!(
                    "Will rebuild from the '{channel}' channel (currently on '{}').",
                    local_branch.unwrap_or("unknown")
                ))
            } else {
                None
            };
            (available, note)
        }
        _ => (
            true,
            Some("Couldn't read the local version — update will re-clone from your selected channel.".into()),
        ),
    }
}

#[derive(Serialize)]
pub struct UpdateInfo {
    /// A newer commit is on GitHub for the active channel and we can update in place.
    pub available: bool,
    /// Short SHA the app was built from (None if unknown).
    pub current: Option<String>,
    /// Short SHA of the latest commit on the active channel.
    pub latest: Option<String>,
    /// First line of the latest commit message ("what's new").
    pub message: String,
    /// ISO date of the latest commit.
    pub date: String,
    /// Whether the in-place installer is present to run the update.
    pub can_self_update: bool,
    /// Human note when we can't determine the local version, etc.
    pub note: Option<String>,
    /// Active update channel (`main` or `dev`).
    pub channel: String,
    /// Local checkout branch name, when known.
    pub local_branch: Option<String>,
}

#[derive(Serialize)]
pub struct ChannelInfo {
    pub channel: String,
    pub local_branch: Option<String>,
    pub current: Option<String>,
    pub can_self_update: bool,
}

#[tauri::command]
pub fn get_update_channel(db: tauri::State<'_, Db>) -> Result<ChannelInfo, String> {
    let channel = active_channel(&db);
    let base = install_base();
    let src = base.join("src");
    let short = |s: &str| s.chars().take(7).collect::<String>();
    Ok(ChannelInfo {
        channel,
        local_branch: local_branch(&src),
        current: local_head(&src).as_deref().map(short),
        can_self_update: base.join("uninstall.exe").exists(),
    })
}

#[tauri::command]
pub fn set_update_channel(db: tauri::State<'_, Db>, channel: String) -> Result<ChannelInfo, String> {
    set_channel(&db, channel.trim())?;
    // Return post-switch identity (branch may already match after ensure_checkout_branch).
    get_update_channel(db)
}

#[tauri::command]
pub async fn check_for_update(db: tauri::State<'_, Db>) -> Result<UpdateInfo, String> {
    let channel = active_channel(&db);
    let base = install_base();
    let src = base.join("src");
    let local = local_head(&src);
    let local_br = local_branch(&src);
    let can_self_update = base.join("uninstall.exe").exists();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("xConsole-updater")
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("https://api.github.com/repos/{REPO}/commits/{channel}");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub check failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "GitHub returned HTTP {} for branch '{channel}'",
            resp.status()
        ));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let latest_sha = v
        .get("sha")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let message = v
        .pointer("/commit/message")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let date = v
        .pointer("/commit/committer/date")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    let (available, note) = update_decision(
        local.as_deref(),
        local_br.as_deref(),
        &channel,
        &latest_sha,
    );
    let available = available && can_self_update;
    let short = |s: &str| s.chars().take(7).collect::<String>();

    Ok(UpdateInfo {
        available,
        current: local.as_deref().map(short),
        latest: (!latest_sha.is_empty()).then(|| short(&latest_sha)),
        message,
        date,
        can_self_update,
        note,
        channel,
        local_branch: local_br,
    })
}

/// Recursively copy a directory tree.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Keep only the newest `keep` backup folders; delete older ones.
fn prune_backups(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // Names are pre-update-YYYYMMDD-HHMMSS, so lexical sort == chronological.
    dirs.sort();
    if dirs.len() > keep {
        for old in &dirs[..dirs.len() - keep] {
            let _ = std::fs::remove_dir_all(old);
        }
    }
}

/// Snapshot the user's data (DB + agent files) to a timestamped backup. Returns the
/// backup path. Belt-and-suspenders before an update — the rebuild never touches the
/// data dir, but a verified pre-update copy means a bad migration can always be undone.
fn backup_user_data(app: &AppHandle) -> Result<PathBuf, String> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dest = data.join("backups").join(format!("pre-update-{stamp}"));
    std::fs::create_dir_all(&dest).map_err(|e| format!("backup mkdir: {e}"))?;

    // When the app lock is on, the at-rest artifacts are the encrypted blob + the lock
    // manifest; otherwise it's the plaintext DB. Back up whichever exists (so we never scatter
    // plaintext copies or block updates on a missing plaintext file).
    let encrypted = data.join("xconsole.db.enc").exists();
    let names: &[&str] = if encrypted {
        &["xconsole.db.enc", "db.lock.json"]
    } else {
        &["xconsole.db", "xconsole.db-wal", "xconsole.db-shm"]
    };
    for name in names {
        let src = data.join(name);
        if src.exists() {
            std::fs::copy(&src, dest.join(name)).map_err(|e| format!("backup {name}: {e}"))?;
        }
    }
    let agent = data.join("agent");
    if agent.exists() {
        copy_dir_all(&agent, &dest.join("agent")).map_err(|e| format!("backup agent files: {e}"))?;
    }

    // Sanity-check the primary DB artifact copied non-empty before we proceed.
    let primary = if encrypted {
        "xconsole.db.enc"
    } else {
        "xconsole.db"
    };
    if data.join(primary).exists()
        && std::fs::metadata(dest.join(primary))
            .map(|m| m.len())
            .unwrap_or(0)
            == 0
    {
        return Err("data backup looks empty — aborting update to protect your data".into());
    }

    prune_backups(&data.join("backups"), KEEP_BACKUPS);
    Ok(dest)
}

/// Back up the user's data, then launch the installer's rebuild-update with its
/// progress window, targeting the **active channel**. Returns the backup path.
#[tauri::command]
pub async fn start_app_update(app: AppHandle, db: tauri::State<'_, Db>) -> Result<String, String> {
    let channel = active_channel(&db);
    // Ensure the installer sees the same channel even if SQLite isn't consulted.
    write_channel_file(&channel);
    // Switch the git tree onto the channel *before* the installer runs, so even an
    // older uninstall.exe that still hardcodes `main` will at least build from a tree
    // that was already checked out to dev (and the new installer will reaffirm it).
    let src = install_base().join("src");
    if src.join(".git").exists() {
        ensure_checkout_branch(&src, &channel)
            .map_err(|e| format!("could not prepare '{channel}' update: {e}"))?;
    }

    let backup = backup_user_data(&app)?;

    let updater = install_base().join("uninstall.exe");
    if !updater.exists() {
        return Err(
            "The xConsole updater wasn't found. Re-run the installer from \
             https://github.com/DemOnJR/xConsole to update."
                .into(),
        );
    }

    // Launch the installer in update mode for this channel. Detached so it outlives
    // this app — the installer stops the running app before swapping the exe.
    // Also set XCONSOLE_UPDATE_BRANCH so any future installer reads the channel
    // even if CLI flags are stripped.
    let mut cmd = std::process::Command::new(&updater);
    cmd.arg("--update");
    cmd.arg("--branch");
    cmd.arg(&channel);
    cmd.env("XCONSOLE_UPDATE_BRANCH", &channel);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008); // DETACHED_PROCESS
    }
    cmd.spawn()
        .map_err(|e| format!("failed to launch the updater: {e}"))?;

    // Close the running app immediately so Windows releases file locks on the binary and build files
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        app_handle.exit(0);
    });

    Ok(backup.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_branch_mismatch_requires_rebuild_even_at_same_tip() {
        let (available, note) = update_decision(Some("abcdef012345"), Some("main"), "dev", "abcdef099999");
        assert!(available);
        assert!(note.unwrap().contains("'dev'"));
    }

    #[test]
    fn matching_branch_and_tip_is_up_to_date() {
        let (available, note) = update_decision(Some("abcdef012345"), Some("dev"), "dev", "abcdef099999");
        assert!(!available);
        assert!(note.is_none());
    }

    #[test]
    fn changed_commit_requires_update_on_matching_branch() {
        let (available, note) = update_decision(Some("111111122222"), Some("dev"), "dev", "333333344444");
        assert!(available);
        assert!(note.is_none());
    }

    #[test]
    fn missing_local_checkout_requires_reclone() {
        let (available, note) = update_decision(None, None, "dev", "abcdef012345");
        assert!(available);
        assert!(note.unwrap().contains("re-clone"));
    }
}
