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
    let rd = std::fs::read_dir(path).map_err(|e| format!("list failed: {e}"))?;
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        match entry.metadata() {
            Ok(m) if m.is_dir() => dirs.push(format!("{name}/")),
            Ok(m) => files.push(format!("{name}  ({} bytes)", m.len())),
            Err(_) => files.push(name),
        }
    }
    dirs.sort();
    files.sort();
    if dirs.is_empty() && files.is_empty() {
        return Ok("(empty)".to_string());
    }
    dirs.extend(files);
    Ok(dirs.join("\n"))
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
