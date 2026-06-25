//! In-app updater for the clone+compile distribution.
//!
//! The app is installed by cloning + compiling from GitHub (see `installer/`). To
//! update, we compare the local checkout's HEAD against `origin/main` on GitHub and,
//! on the user's accept, re-run the installer — which does `git fetch + reset --hard`,
//! rebuilds, and swaps in the new exe. The installer self-copies to
//! `%LOCALAPPDATA%\xConsole\uninstall.exe`, so it's always available to re-invoke.
//!
//! USER DATA IS SAFE: it lives in the app-data dir (`%APPDATA%\com.xconsole.app`:
//! `xconsole.db` = chats/VPS/providers/workspaces/settings/cron, and `agent\` =
//! memory/soul/skills) and the OS keychain (credentials) — a *different* tree from the
//! app binary, so the rebuild can't touch it. As an extra safety net we also snapshot
//! the DB + agent files to a timestamped backup BEFORE launching the update.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

const REPO: &str = "DemOnJR/xConsole";
const BRANCH: &str = "main";
const KEEP_BACKUPS: usize = 5;

/// Where the installer placed the app (and itself). Mirrors the installer's `base_dir`.
fn install_base() -> PathBuf {
    if let Ok(b) = std::env::var("XCONSOLE_INSTALL_BASE") {
        if !b.trim().is_empty() {
            return PathBuf::from(b);
        }
    }
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("xConsole")
}

/// The SHA the local source is checked out at (depth-1 clone of `main`).
fn local_head(src: &Path) -> Option<String> {
    let git = src.join(".git");
    if let Ok(s) = std::fs::read_to_string(git.join("refs").join("heads").join(BRANCH)) {
        let s = s.trim();
        if s.len() >= 7 {
            return Some(s.to_string());
        }
    }
    // Fallback: a packed-refs entry "<sha> refs/heads/main".
    if let Ok(packed) = std::fs::read_to_string(git.join("packed-refs")) {
        for line in packed.lines() {
            if line.trim_end().ends_with(&format!("refs/heads/{BRANCH}")) {
                if let Some(sha) = line.split_whitespace().next() {
                    return Some(sha.to_string());
                }
            }
        }
    }
    None
}

#[derive(Serialize)]
pub struct UpdateInfo {
    /// A newer commit is on GitHub and we can update in place.
    pub available: bool,
    /// Short SHA the app was built from (None if unknown).
    pub current: Option<String>,
    /// Short SHA of the latest commit on GitHub.
    pub latest: Option<String>,
    /// First line of the latest commit message ("what's new").
    pub message: String,
    /// ISO date of the latest commit.
    pub date: String,
    /// Whether the in-place installer is present to run the update.
    pub can_self_update: bool,
    /// Human note when we can't determine the local version, etc.
    pub note: Option<String>,
}

#[tauri::command]
pub async fn check_for_update() -> Result<UpdateInfo, String> {
    let base = install_base();
    let local = local_head(&base.join("src"));
    let can_self_update = base.join("uninstall.exe").exists();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("xConsole-updater")
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("https://api.github.com/repos/{REPO}/commits/{BRANCH}");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub check failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub returned HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let latest_sha = v.get("sha").and_then(|s| s.as_str()).unwrap_or("").to_string();
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

    let short = |s: &str| s.chars().take(7).collect::<String>();
    let (available, note) = match &local {
        Some(l) if !latest_sha.is_empty() => (*l != latest_sha, None),
        _ => (
            false,
            Some("Couldn't read the local version — update by re-running the installer.".to_string()),
        ),
    };

    Ok(UpdateInfo {
        available,
        current: local.as_deref().map(short),
        latest: (!latest_sha.is_empty()).then(|| short(&latest_sha)),
        message,
        date,
        can_self_update,
        note,
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
    let Ok(rd) = std::fs::read_dir(dir) else { return };
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
    let primary = if encrypted { "xconsole.db.enc" } else { "xconsole.db" };
    if data.join(primary).exists()
        && std::fs::metadata(dest.join(primary)).map(|m| m.len()).unwrap_or(0) == 0
    {
        return Err("data backup looks empty — aborting update to protect your data".into());
    }

    prune_backups(&data.join("backups"), KEEP_BACKUPS);
    Ok(dest)
}

/// Back up the user's data, then launch the installer's rebuild-update with its
/// progress window. Returns the backup path on success.
#[tauri::command]
pub async fn start_app_update(app: AppHandle) -> Result<String, String> {
    let backup = backup_user_data(&app)?;

    let updater = install_base().join("uninstall.exe");
    if !updater.exists() {
        return Err(
            "The xConsole updater wasn't found. Re-run the installer from \
             https://github.com/DemOnJR/xConsole to update."
                .into(),
        );
    }

    // Launch the installer in update mode (shows its build-progress window), detached so
    // it outlives this app — the installer stops the running app before swapping the exe.
    let mut cmd = std::process::Command::new(&updater);
    cmd.arg("--update");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008); // DETACHED_PROCESS
    }
    cmd.spawn().map_err(|e| format!("failed to launch the updater: {e}"))?;

    Ok(backup.to_string_lossy().into_owned())
}
