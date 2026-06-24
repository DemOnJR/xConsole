//! Remote file operations via SSH exec (chmod -R, rm, mv, etc.) — not SFTP per-file.

use serde::Serialize;

use super::manager::{CommandOutput, SessionManager};

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn validate_remote_path(path: &str) -> Result<String, String> {
    let p = path.trim();
    if p.is_empty() {
        return Err("path is empty".into());
    }
    if !p.starts_with('/') {
        return Err("path must be absolute".into());
    }
    if p.contains('\0') || p.contains('\n') || p.contains('\r') {
        return Err("invalid path".into());
    }
    Ok(p.to_string())
}

pub fn validate_octal_mode(mode: &str) -> Result<String, String> {
    let m = mode.trim();
    if m.is_empty() || m.len() > 4 {
        return Err("invalid mode".into());
    }
    if !m.chars().all(|c| c.is_ascii_digit() && c <= '7') {
        return Err("mode must be octal (0-7)".into());
    }
    Ok(m.to_string())
}

fn validate_owner_part(s: &str, label: &str) -> Result<String, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err(format!("{label} is empty"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(format!("invalid {label}"));
    }
    Ok(s.to_string())
}

fn command_err(out: CommandOutput) -> String {
    let msg = out.stderr.trim();
    if msg.is_empty() {
        out.stdout.trim().to_string()
    } else {
        msg.to_string()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteFileStat {
    pub mode: String,
    pub owner: String,
    pub group: String,
    pub is_dir: bool,
}

pub async fn stat_file(
    sessions: &SessionManager,
    vps_id: &str,
    path: &str,
) -> Result<RemoteFileStat, String> {
    let path = validate_remote_path(path)?;
    let cmd = format!("stat -c '%a %U %G %F' -- {}", shell_quote(&path));
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    let line = out.stdout.lines().next().unwrap_or("").trim();
    let mut parts = line.split_whitespace();
    let mode = parts.next().ok_or("stat parse failed")?.to_string();
    let owner = parts.next().ok_or("stat parse failed")?.to_string();
    let group = parts.next().ok_or("stat parse failed")?.to_string();
    let kind = parts.collect::<Vec<_>>().join(" ");
    let is_dir = kind.contains("directory");
    Ok(RemoteFileStat {
        mode,
        owner,
        group,
        is_dir,
    })
}

pub async fn chmod(
    sessions: &SessionManager,
    vps_id: &str,
    path: &str,
    mode: &str,
    recursive: bool,
) -> Result<CommandOutput, String> {
    let path = validate_remote_path(path)?;
    let mode = validate_octal_mode(mode)?;
    let flag = if recursive { "-R " } else { "" };
    let cmd = format!("chmod {flag}{mode} -- {}", shell_quote(&path));
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

pub async fn chown(
    sessions: &SessionManager,
    vps_id: &str,
    path: &str,
    owner: &str,
    group: &str,
    recursive: bool,
) -> Result<CommandOutput, String> {
    let path = validate_remote_path(path)?;
    let owner = validate_owner_part(owner, "owner")?;
    let group = validate_owner_part(group, "group")?;
    let flag = if recursive { "-R " } else { "" };
    let spec = format!("{owner}:{group}");
    let cmd = format!("chown {flag}{} -- {}", shell_quote(&spec), shell_quote(&path));
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

pub async fn delete_path(
    sessions: &SessionManager,
    vps_id: &str,
    path: &str,
    is_dir: bool,
) -> Result<CommandOutput, String> {
    let path = validate_remote_path(path)?;
    let cmd = if is_dir {
        format!("rm -rf -- {}", shell_quote(&path))
    } else {
        format!("rm -f -- {}", shell_quote(&path))
    };
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

pub async fn rename_path(
    sessions: &SessionManager,
    vps_id: &str,
    from: &str,
    to: &str,
) -> Result<CommandOutput, String> {
    let from = validate_remote_path(from)?;
    let to = validate_remote_path(to)?;
    let cmd = format!(
        "mv -- {} {}",
        shell_quote(&from),
        shell_quote(&to)
    );
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

pub async fn mkdir_path(
    sessions: &SessionManager,
    vps_id: &str,
    path: &str,
) -> Result<CommandOutput, String> {
    let path = validate_remote_path(path)?;
    let cmd = format!("mkdir -p -- {}", shell_quote(&path));
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

pub async fn touch_file(
    sessions: &SessionManager,
    vps_id: &str,
    path: &str,
) -> Result<CommandOutput, String> {
    let path = validate_remote_path(path)?;
    let cmd = format!("touch -- {}", shell_quote(&path));
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}
