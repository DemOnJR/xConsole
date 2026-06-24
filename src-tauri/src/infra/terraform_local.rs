//! Run Terraform on the local agent host (no VPS).

use std::collections::HashMap;

use tokio::process::Command;

use crate::ai::AgentHome;
use crate::infra::projects::project_dir;

pub struct LocalOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Human-readable command line for the safety gate (never includes secrets).
pub fn describe_command(slug: &str, subcommand: &str, args: &[String]) -> String {
    if args.is_empty() {
        format!("local terraform {subcommand} (project: {slug})")
    } else {
        format!("local terraform {subcommand} {} (project: {slug})", args.join(" "))
    }
}

pub async fn run_local(
    home: &AgentHome,
    slug: &str,
    subcommand: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<LocalOutput, String> {
    let dir = project_dir(home, slug);
    if !dir.exists() {
        return Err(format!("project directory missing: {}", dir.display()));
    }

    let mut cmd = Command::new("terraform");
    cmd.current_dir(&dir);
    cmd.arg(subcommand);
    // Pass each token as a distinct argv entry (no shell), so values with
    // spaces/quotes are preserved verbatim.
    for token in args {
        cmd.arg(token);
    }
    cmd.envs(env);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "terraform not found in PATH; install Terraform on this machine or use runner=vps".to_string()
        } else {
            e.to_string()
        }
    })?;

    Ok(LocalOutput {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Stable path for a GCP service-account JSON file on the agent host.
pub fn gcp_cred_path(home: &AgentHome, account_id: &str) -> std::path::PathBuf {
    home.0
        .join(".cloud-creds")
        .join(format!("gcp-{account_id}.json"))
}

pub fn write_gcp_cred_file(home: &AgentHome, account_id: &str, secret: &str) -> Result<std::path::PathBuf, String> {
    let path = gcp_cred_path(home, account_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, secret.trim()).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_command_includes_slug() {
        let s = describe_command("my-app", "plan", &["-out=tfplan".to_string()]);
        assert!(s.contains("my-app"));
        assert!(s.contains("plan"));
        assert!(s.contains("-out=tfplan"));
    }
}
