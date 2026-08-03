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

/// Delete several entries in one command.
///
/// `rm -rf` takes a list, so a selection is one round trip rather than one per file. The
/// paths are validated individually first: a single bad one must not be quietly skipped
/// while the rest are removed.
pub async fn delete_many(
    sessions: &SessionManager,
    vps_id: &str,
    paths: &[String],
) -> Result<CommandOutput, String> {
    if paths.is_empty() {
        return Err("nothing selected".into());
    }
    let mut args = String::new();
    for path in paths {
        let path = validate_remote_path(path)?;
        if path == "/" {
            return Err("refusing to delete /".into());
        }
        args.push(' ');
        args.push_str(&shell_quote(&path));
    }
    let out = sessions.run_command(vps_id, &format!("rm -rf --{args}")).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

/// Copy or move several entries into a directory, in one command.
///
/// One `cp`/`mv` invocation rather than one per file: a selection of two hundred files
/// would otherwise be two hundred SSH round trips, and the shell already accepts a list
/// followed by a destination directory. `-R` so directories come along; POSIX spells it
/// with a capital, and GNU accepts that too.
pub async fn copy_into(
    sessions: &SessionManager,
    vps_id: &str,
    sources: &[String],
    dest_dir: &str,
    move_them: bool,
) -> Result<CommandOutput, String> {
    if sources.is_empty() {
        return Err("nothing selected".into());
    }
    let dest = validate_remote_path(dest_dir)?;
    let mut args = String::new();
    for src in sources {
        let src = validate_remote_path(src)?;
        // Moving a directory into itself, or copying it into itself, is a way to lose it.
        if dest == src || dest.starts_with(&format!("{src}/")) {
            return Err(format!("cannot put {src} inside itself"));
        }
        args.push_str(&shell_quote(&src));
        args.push(' ');
    }
    let tool = if move_them { "mv --" } else { "cp -R --" };
    let cmd = format!("{tool} {args}{}", shell_quote(&dest));
    let out = sessions.run_command(vps_id, &cmd).await?;
    if out.exit_code != 0 {
        return Err(command_err(out));
    }
    Ok(out)
}

/// How deep a recursive search will look, and how many hits it will return.
///
/// Both are bounds on a command running on someone's production server: an unbounded
/// `find /` on a large filesystem is a long stall and a lot of I/O, and a result list
/// nobody can read is not worth the wait.
const SEARCH_MAX_DEPTH: u32 = 12;
const SEARCH_MAX_RESULTS: u32 = 500;

/// Find entries under `root` whose name matches, optionally restricted to extensions.
///
/// Deliberately tolerant of a non-zero exit. `find` returns failure if it could not read
/// *any* directory along the way — which on a real server means almost always, since a
/// search from `/` will meet something it may not read — and `head` closing the pipe kills
/// it with SIGPIPE besides. Neither means the results so far are wrong, so stdout is
/// parsed regardless and only a genuinely empty result is empty.
///
/// No redirections and no backslashes: this string is handed to the account's login shell,
/// which on FreeBSD is csh, and csh understands neither `2>/dev/null` nor `\(`. The
/// parentheses are passed as quoted arguments instead, which both shells hand to `find`
/// unchanged.
pub async fn search(
    sessions: &SessionManager,
    vps_id: &str,
    root: &str,
    pattern: &str,
    extensions: &[String],
    recursive: bool,
) -> Result<Vec<String>, String> {
    let root = validate_remote_path(root)?;
    let pattern = pattern.trim();
    if pattern.is_empty() && extensions.is_empty() {
        return Ok(Vec::new());
    }
    let depth = if recursive { SEARCH_MAX_DEPTH } else { 1 };

    // A bare pattern is matched as a substring, which is what people expect from a search
    // box; a pattern already carrying a wildcard is taken as written.
    let glob = |p: &str| {
        if p.contains('*') || p.contains('?') {
            p.to_string()
        } else {
            format!("*{p}*")
        }
    };

    let mut matcher = String::new();
    if extensions.is_empty() {
        matcher.push_str(&format!("-iname {}", shell_quote(&glob(pattern))));
    } else {
        // Every extension is its own -iname, OR-ed inside parentheses so the whole group
        // binds as one condition rather than the last one winning.
        matcher.push_str(&format!("{} ", shell_quote("(")));
        for (i, ext) in extensions.iter().enumerate() {
            let ext = ext.trim().trim_start_matches('.');
            if ext.is_empty() {
                continue;
            }
            if i > 0 {
                matcher.push_str("-o ");
            }
            let pat = if pattern.is_empty() {
                format!("*.{ext}")
            } else {
                format!("{}.{ext}", glob(pattern).trim_end_matches('*'))
            };
            matcher.push_str(&format!("-iname {} ", shell_quote(&pat)));
        }
        matcher.push_str(&shell_quote(")"));
    }

    let cmd = format!(
        "find {} -maxdepth {depth} {matcher} -print | head -n {SEARCH_MAX_RESULTS}",
        shell_quote(&root)
    );
    let out = sessions.run_command(vps_id, &cmd).await?;
    Ok(out
        .stdout
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// A symlink target, which — unlike every other path here — may legitimately be relative.
///
/// `../releases/v3` and `bin/app` are ordinary, correct targets; requiring absolute paths
/// would refuse to create the kind of link most deployment layouts are built from. So this
/// checks only what would be unsafe or unrepresentable: nothing empty, and nothing
/// carrying a NUL or a newline. Quoting is left to [`shell_quote`], as everywhere else.
pub fn validate_link_target(target: &str) -> Result<String, String> {
    let t = target.trim();
    if t.is_empty() {
        return Err("link target is empty".into());
    }
    if t.contains('\0') || t.contains('\n') || t.contains('\r') {
        return Err("invalid link target".into());
    }
    Ok(t.to_string())
}

/// Create a symlink, or repoint an existing one.
///
/// `ln -sfn` is the one form that does both and is correct on GNU and BSD alike: `-f`
/// replaces what is there, and `-n` (`-h` on the BSDs, which accept `-n` as its synonym)
/// stops it from dereferencing a link that already points at a directory — without it,
/// repointing `current -> releases/v1` would create `releases/v1/current` instead of
/// changing the link, which is the classic way to make a mess of a deployment tree.
pub async fn symlink(
    sessions: &SessionManager,
    vps_id: &str,
    link_path: &str,
    target: &str,
) -> Result<CommandOutput, String> {
    let link_path = validate_remote_path(link_path)?;
    let target = validate_link_target(target)?;
    let cmd = format!(
        "ln -sfn -- {} {}",
        shell_quote(&target),
        shell_quote(&link_path)
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

#[cfg(test)]
mod symlink_tests {
    use super::{shell_quote, validate_link_target, validate_remote_path};

    /// A link target is the one path here that may be relative, and refusing relative
    /// targets would refuse the layout most deployments are built from:
    /// `current -> releases/v3`.
    #[test]
    fn a_relative_target_is_accepted_where_a_path_would_not_be() {
        assert_eq!(validate_link_target("../releases/v3").unwrap(), "../releases/v3");
        assert_eq!(validate_link_target("bin/app").unwrap(), "bin/app");
        // ...while the link itself still has to be absolute.
        assert!(validate_remote_path("../releases/v3").is_err());
    }

    #[test]
    fn an_absolute_target_is_fine_too() {
        assert_eq!(validate_link_target("/srv/app/v3").unwrap(), "/srv/app/v3");
    }

    /// Empty, and anything carrying a NUL or a newline — the two things a shell command
    /// line cannot carry safely no matter how it is quoted.
    #[test]
    fn unrepresentable_targets_are_refused() {
        assert!(validate_link_target("").is_err());
        assert!(validate_link_target("   ").is_err());
        assert!(validate_link_target("a\nb").is_err());
        assert!(validate_link_target("a\0b").is_err());
        assert!(validate_link_target("a\rb").is_err());
    }

    /// The target reaches the shell as one argument whatever is in it.
    ///
    /// Pinned to the exact bytes rather than a substring check: the escape idiom is
    /// `'\''`, and a version of this helper that emits `\'` instead leaves the quote
    /// open and turns any target into a command. That mistake has been made in this
    /// codebase before, and it is invisible unless the output is compared literally.
    #[test]
    fn a_hostile_target_is_quoted_exactly() {
        assert_eq!(
            shell_quote("/tmp/x'; rm -rf / #"),
            "'/tmp/x'\\''; rm -rf / #'"
        );
        // Nothing to escape: still one argument.
        assert_eq!(shell_quote("/srv/app"), "'/srv/app'");
    }
}
