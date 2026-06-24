//! Sync local projects to a VPS and run Terraform there.

use base64::Engine;

use crate::ai::AgentHome;
use crate::infra::projects::{list_project_files, read_project_file, slugify};
use crate::ssh::{shell_quote, SessionManager};
use crate::storage::Db;

const REMOTE_ROOT: &str = "$HOME/xconsole-projects";

/// Remote directory for a project on the VPS.
pub fn remote_project_dir(slug: &str) -> String {
    format!("{REMOTE_ROOT}/{}", slugify(slug))
}

/// Shell command to ensure terraform exists on the runner.
pub fn ensure_terraform_cmd() -> &'static str {
    "command -v terraform >/dev/null || (\
      curl -fsSL https://releases.hashicorp.com/terraform/1.9.8/terraform_1.9.8_linux_amd64.zip -o /tmp/tf.zip \
      && unzip -qo /tmp/tf.zip -d \"$HOME/.local/bin\" \
      && rm /tmp/tf.zip)"
}

/// Build the command that syncs all project files to the VPS via base64 tar.
pub fn build_sync_command(home: &AgentHome, slug: &str) -> Result<String, String> {
    let files = list_project_files(home, slug)?;
    if files.is_empty() {
        return Err("project has no files to sync".into());
    }
    let remote = remote_project_dir(slug);
    let mut tar_parts = Vec::new();
    for rel in &files {
        let content = read_project_file(home, slug, rel)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
        let rel_q = shell_quote(rel);
        tar_parts.push(format!(
            "mkdir -p {remote}/$(dirname {rel_q}) && printf %s {} | base64 -d > {remote}/{rel_q}",
            shell_quote(&b64),
            remote = remote,
            rel_q = rel_q
        ));
    }
    Ok(tar_parts.join(" && "))
}

/// Full remote command: sync, credentials, ensure terraform, run subcommand.
/// `args` are raw, unquoted terraform tokens; each is shell-quoted here for the
/// remote shell.
pub fn build_remote_terraform_command(
    home: &AgentHome,
    slug: &str,
    subcommand: &str,
    args: &[String],
    credential_prefix: Option<&str>,
) -> Result<String, String> {
    let sync = build_sync_command(home, slug)?;
    let remote = remote_project_dir(slug);
    let tf = ensure_terraform_cmd();
    let run = if args.is_empty() {
        format!("cd {remote} && terraform {subcommand}")
    } else {
        let quoted = args.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
        format!("cd {remote} && terraform {subcommand} {quoted}")
    };
    let mut parts = vec![sync];
    if let Some(creds) = credential_prefix.filter(|s| !s.is_empty()) {
        parts.push(creds.to_string());
    }
    parts.push(tf.to_string());
    parts.push(run);
    Ok(parts.join(" && "))
}

/// Whether a terraform subcommand is read-only (safe for allowlist auto-run).
pub fn is_readonly_subcommand(sub: &str) -> bool {
    matches!(
        sub.trim().to_lowercase().as_str(),
        "plan" | "validate" | "fmt" | "show" | "version" | "providers" | "output"
    )
}

/// Raw `-var=vps_*` terraform tokens from the linked VPS record (unquoted; the
/// runner quotes per-token — argv for local, shell_quote for remote).
pub async fn vps_var_args(db: &Db, vps_id: &str) -> Result<Vec<String>, String> {
    let vps = db
        .get_vps(vps_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("VPS '{vps_id}' not found"))?;
    Ok(vec![
        format!("-var=vps_host={}", vps.host),
        format!("-var=vps_user={}", vps.username),
        format!("-var=vps_port={}", vps.port),
    ])
}

pub async fn run_on_vps(
    sessions: &SessionManager,
    vps_id: &str,
    command: &str,
) -> Result<crate::ssh::manager::CommandOutput, String> {
    sessions.run_command(vps_id, command).await
}

pub fn summarize_plan(stdout: &str) -> String {
    let mut summary = String::new();
    for line in stdout.lines() {
        let t = line.trim();
        if t.starts_with("Plan:")
            || t.starts_with("No changes.")
            || t.contains("to add,")
            || t.contains("to change,")
            || t.contains("to destroy.")
        {
            summary.push_str(line);
            summary.push('\n');
        }
    }
    if summary.is_empty() {
        stdout.chars().take(2000).collect()
    } else {
        summary
    }
}
