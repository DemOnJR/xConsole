//! Local-machine execution and file operations.
//!
//! This is the local-PC counterpart to the SSH command/file path
//! ([`crate::ssh::command`]): it lets the agent do the same jobs (run commands,
//! read/write files) on the user's own machine instead of a remote VPS. The
//! agent tool layer ([`crate::ai::tools`]) routes these through the SAME safety
//! gate ([`crate::ai::safety`]) as SSH commands, so nothing runs locally without
//! the user's approval unless they lower the safety mode.

use std::time::Duration;

use crate::ssh::manager::CommandOutput;

/// Maximum wall-clock time for a local command, matching the SSH path
/// ([`crate::ssh::command::COMMAND_TIMEOUT`]).
pub const LOCAL_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Run a shell command on the local machine and capture stdout/stderr/exit code.
/// Returns the same [`CommandOutput`] shape as the SSH path so the tool layer can
/// format both identically.
pub async fn run_local_command(command: &str) -> Result<CommandOutput, String> {
    match tokio::time::timeout(LOCAL_COMMAND_TIMEOUT, run_local_command_inner(command)).await {
        Ok(r) => r,
        Err(_) => Err(format!(
            "command timed out after {}s",
            LOCAL_COMMAND_TIMEOUT.as_secs()
        )),
    }
}

async fn run_local_command_inner(command: &str) -> Result<CommandOutput, String> {
    use tokio::process::Command;

    // Run through the platform's default shell so pipes/globs/builtins work the
    // same way the user would type them. The command is passed as a single
    // argument — no extra interpolation beyond what the caller supplied.
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };

    crate::proc::hide_console(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to spawn shell: {e}"))?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

/// Read a UTF-8 text file from the local filesystem.
pub fn read_local_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))
}

/// Write (overwrite) a local text file, creating parent directories as needed —
/// mirroring the `mkdir -p` behavior of the VPS `write_file` tool.
pub fn write_local_file(path: &str, content: &str) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
        }
    }
    std::fs::write(path, content).map_err(|e| format!("write failed: {e}"))
}

/// List a local directory as a human-readable listing (dirs first, then files
/// with byte sizes), for the agent's `local_list_dir` tool.
pub fn list_local_dir(path: &str) -> Result<String, String> {
    let listing = list_local_dir_entries(path)?;
    if listing.entries.is_empty() {
        return Ok("(empty)".to_string());
    }
    let mut lines: Vec<String> = Vec::new();
    for e in &listing.entries {
        if e.is_dir {
            lines.push(format!("{}/", e.name));
        } else {
            lines.push(format!("{}  ({} bytes)", e.name, e.size));
        }
    }
    Ok(lines.join("\n"))
}

/// One entry in a local directory listing (UI / dual-pane SFTP).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalFsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalFsList {
    pub path: String,
    pub entries: Vec<LocalFsEntry>,
}

/// Git branch for a local path when it is inside a work tree.
pub fn local_git_branch(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return None;
    }
    let out = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let br = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !br.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&br.stdout).trim().to_string();
    if name.is_empty() {
        return None;
    }
    if name == "HEAD" {
        let short = std::process::Command::new("git")
            .args(["-C", path, "rev-parse", "--short", "HEAD"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&short.stdout).trim().to_string();
        if s.is_empty() {
            return None;
        }
        return Some(format!("detached@{s}"));
    }
    Some(name)
}

/// Structured local directory list for the dual-pane file manager.
pub fn list_local_dir_entries(path: &str) -> Result<LocalFsList, String> {
    let p = std::path::PathBuf::from(path);
    let canonical = if p.as_os_str().is_empty() {
        dirs::home_dir().ok_or_else(|| "could not resolve home directory".to_string())?
    } else {
        p.canonicalize().unwrap_or(p)
    };
    let rd = std::fs::read_dir(&canonical).map_err(|e| format!("list failed: {e}"))?;
    let mut entries: Vec<LocalFsEntry> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Skip Windows special/dot noise only when hidden is not useful for a manager
        // (still show .git etc. — power users want them).
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let full = entry.path().to_string_lossy().into_owned();
        entries.push(LocalFsEntry {
            name,
            path: full,
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(LocalFsList {
        path: canonical.to_string_lossy().into_owned(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_local_command_captures_stdout() {
        // `echo hello` works in both PowerShell and sh.
        let out = run_local_command("echo hello").await.expect("command ran");
        assert!(out.stdout.contains("hello"), "stdout was: {:?}", out.stdout);
        assert_eq!(out.exit_code, 0);
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = std::env::temp_dir().join("xconsole_local_test");
        let path = dir.join("hello.txt");
        let p = path.to_string_lossy().to_string();
        write_local_file(&p, "hi there").expect("write");
        assert_eq!(read_local_file(&p).expect("read"), "hi there");
        let _ = std::fs::remove_file(&path);
    }
}
