//! Live-canvas context: the terminals and SFTP panels the user currently has open.
//!
//! The frontend is the source of truth for the canvas — the backend can't
//! enumerate open SFTP panels, doesn't track a terminal's working directory, and
//! has no "all live sessions" view — so the frontend sends a snapshot of the open
//! nodes with each turn. For terminals we then enrich the listing with a tail of
//! the live scrollback (which the backend DOES keep, in the SSH session ring
//! buffer) so the agent can literally see what's on screen without a tool call.

use serde::Deserialize;

use crate::ssh::SessionManager;

/// One open canvas node, as reported by the frontend.
#[derive(Debug, Clone, Deserialize)]
pub struct CanvasNode {
    /// Canvas node id — lets the agent address one specific panel (e.g. to close it).
    #[serde(default)]
    pub node_id: String,
    /// "terminal" | "sftp"
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub vps_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    /// Backend SSH session id (terminals only) — used to fetch live scrollback.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Connection status as shown in the UI (connected / connecting / error / …).
    #[serde(default)]
    pub status: Option<String>,
    /// Terminal working directory (frontend-tracked via OSC 7 / cd parsing).
    #[serde(default)]
    pub cwd: Option<String>,
    /// SFTP panel's current remote path.
    #[serde(default)]
    pub path: Option<String>,
}

/// Max chars of recent terminal output shown per terminal.
const TERM_TAIL_CHARS: usize = 1400;
/// Overall cap on the assembled canvas block.
const BLOCK_MAX_CHARS: usize = 6000;
/// Max parsed commands listed per terminal.
const MAX_COMMANDS: usize = 25;
/// Max length of a single listed command.
const CMD_MAX_CHARS: usize = 240;

/// Extract the commands the user ran from a terminal's scrollback, by reading the
/// text typed after each shell prompt. Returns them oldest→newest. Best-effort: it
/// recognises the common `user@host:cwd$`/`#`/`%`, `[user@host cwd]#`, and starship
/// `❯` prompts; output lines that don't look like a prompt are ignored.
fn extract_commands(scrollback: &str) -> Vec<String> {
    let mut cmds = Vec::new();
    for raw in scrollback.lines() {
        if let Some(cmd) = command_after_prompt(raw.trim_end()) {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                cmds.push(clip(cmd, CMD_MAX_CHARS));
            }
        }
    }
    cmds
}

/// If `line` is a shell prompt with a command typed after it, return that command.
fn command_after_prompt(line: &str) -> Option<&str> {
    // Starship / bare unicode prompt at line start: "❯ cmd".
    if let Some(rest) = line.strip_prefix('❯').and_then(|r| r.strip_prefix(' ')) {
        return Some(rest);
    }
    // Conventional PS1: a prompt symbol (`#`/`$`/`%`) that follows a "user@host:cwd"
    // or "[user@host cwd]" looking prefix, then a space and the command. Requiring
    // `@`+`:` (or `]`) in the prefix keeps arbitrary output from matching.
    for (i, ch) in line.char_indices() {
        if matches!(ch, '#' | '$' | '%') {
            let prefix = &line[..i];
            let prompt_like = (prefix.contains('@') && prefix.contains(':')) || prefix.contains(']');
            if !prompt_like {
                continue;
            }
            if let Some(cmd) = line[i + ch.len_utf8()..].strip_prefix(' ') {
                return Some(cmd);
            }
            // Symbol not followed by a space (e.g. `$` inside a path) — keep scanning.
        }
    }
    None
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

/// Assemble the "Live canvas" context block, or `None` when nothing is open.
pub fn build_canvas_block(nodes: &[CanvasNode], sessions: &SessionManager) -> Option<String> {
    let has_panel = nodes
        .iter()
        .any(|n| n.kind == "terminal" || n.kind == "sftp");
    if !has_panel {
        return None;
    }

    let mut parts: Vec<String> = vec![
        "# Live canvas (what the user has open right now)".to_string(),
        "These are the terminals and SFTP panels currently open on the user's canvas. Treat the \
         terminal output below as exactly what the user is looking at — answer about it directly \
         instead of saying you can't see their screen. Each terminal lists the exact commands the \
         user ran (parsed from the shell prompt); when asked what they ran, trust that 'commands' \
         list rather than guessing from the raw output. To read a terminal's full scrollback call \
         terminal_capture; to type/run a command in it call terminal_send. To edit a file the user \
         is browsing in an SFTP panel, use read_file / write_file with that panel's path as the \
         directory (they act on the same server). You can also open your own terminal with \
         canvas_open_terminal and work in it, reconnect a disconnected terminal with canvas_refresh \
         (e.g. after the user reboots the server), and close a specific panel the user points to with \
         canvas_close (pass its node_id). Use each panel's vps_id as the tool target."
            .to_string(),
    ];

    for n in nodes {
        match n.kind.as_str() {
            "terminal" => {
                let mut head = format!(
                    "## Terminal — {} ({})",
                    blank_to(&n.name, "server"),
                    blank_to(&n.host, &n.vps_id),
                );
                if let Some(st) = n.status.as_deref().filter(|s| !s.is_empty()) {
                    head.push_str(&format!(" [{st}]"));
                }
                head.push_str(&format!("\nvps_id: {} · node_id: {}", n.vps_id, n.node_id));
                if let Some(cwd) = n.cwd.as_deref().filter(|s| !s.is_empty()) {
                    head.push_str(&format!("\ncwd: {cwd}"));
                }
                let full = n
                    .session_id
                    .as_deref()
                    .and_then(|sid| sessions.capture_text(sid))
                    .unwrap_or_default();
                // Explicit, parsed command history so the agent doesn't have to
                // infer commands from raw prompt/echo/output text.
                let commands = extract_commands(&full);
                if !commands.is_empty() {
                    let start = commands.len().saturating_sub(MAX_COMMANDS);
                    head.push_str("\ncommands the user ran (oldest first):");
                    for c in &commands[start..] {
                        head.push_str(&format!("\n  $ {c}"));
                    }
                }
                let body = tail(full.trim_end(), TERM_TAIL_CHARS);
                if body.trim().is_empty() {
                    head.push_str("\n(no output captured yet)");
                } else {
                    head.push_str(&format!("\nrecent output:\n```\n{body}\n```"));
                }
                parts.push(head);
            }
            "sftp" => {
                let mut head = format!(
                    "## SFTP — {} ({})",
                    blank_to(&n.name, "server"),
                    blank_to(&n.host, &n.vps_id),
                );
                head.push_str(&format!("\nvps_id: {} · node_id: {}", n.vps_id, n.node_id));
                let p = n.path.as_deref().filter(|s| !s.is_empty()).unwrap_or("/");
                head.push_str(&format!("\nbrowsing: {p}"));
                parts.push(head);
            }
            _ => {}
        }
    }

    Some(truncate(&parts.join("\n\n"), BLOCK_MAX_CHARS))
}

fn blank_to(s: &str, fallback: &str) -> String {
    if s.trim().is_empty() {
        fallback.to_string()
    } else {
        s.trim().to_string()
    }
}

/// Keep the last `max` chars (on a char boundary), marking the cut.
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let start = s.len() - max;
    let cut = (start..s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(start);
    format!("…(earlier output trimmed)\n{}", &s[cut..])
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n…(truncated)", &s[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_to_uses_fallback() {
        assert_eq!(blank_to("  ", "server"), "server");
        assert_eq!(blank_to(" web ", "server"), "web");
    }

    #[test]
    fn tail_keeps_recent_chars() {
        assert_eq!(tail("hello", 10), "hello");
        let t = tail("0123456789abcdef", 4);
        assert!(t.ends_with("cdef"));
        assert!(t.contains("trimmed"));
    }

    #[test]
    fn truncate_marks_cut() {
        assert_eq!(truncate("short", 10), "short");
        assert!(truncate("0123456789", 4).contains("truncated"));
    }

    #[test]
    fn extract_commands_reads_bash_prompts() {
        let sb = "\
root@ubuntu:~# ls
gamequery-k8s  pbcv-k8s  dashboard
root@ubuntu:~# ps x
  PID TTY      STAT   TIME COMMAND
 1234 pts/1    Ss     0:00 -bash
root@ubuntu:~# ";
        assert_eq!(extract_commands(sb), vec!["ls", "ps x"]);
    }

    #[test]
    fn extract_commands_handles_paths_and_brackets() {
        let sb = "\
user@host:/var/www$ echo $PATH
/usr/bin
[root@host ~]# systemctl status nginx
total 0";
        assert_eq!(
            extract_commands(sb),
            vec!["echo $PATH", "systemctl status nginx"],
        );
    }

    #[test]
    fn extract_commands_ignores_plain_output() {
        // No prompt-looking lines → no commands.
        let sb = "Reading package lists... Done\nBuilding dependency tree";
        assert!(extract_commands(sb).is_empty());
    }
}
