//! Run an agent CLI on a VPS, with xConsole's tools tunnelled back to it.
//!
//! [`super::command::run_vps_command`] is the wrong tool for this: it buffers the whole
//! output and gives up after two minutes. An agent turn streams for as long as the work
//! takes, and the caller needs each line as it arrives so the desktop shows the run
//! rather than a frozen spinner.
//!
//! The other half is reach. An agent on the VPS should be able to use xConsole's own
//! tools — SSH to the user's *other* servers, memory, skills, the safety gate — not just
//! the box it happens to be sitting on. It cannot spawn xConsole as a subprocess from
//! there, so instead:
//!
//! 1. xConsole serves MCP on a loopback port here ([`crate::mcp::http`]).
//! 2. It asks the SSH server for a reverse forward, giving the VPS a `127.0.0.1` port.
//! 3. It hands the agent that URL and a one-run bearer token.
//!
//! Nothing listens on a public interface at either end, and the connection carrying it
//! was dialled outbound by this app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use russh::{ChannelMsg, Disconnect};

use crate::storage::Db;

use super::client;
use super::remote_ops::shell_quote;
use super::tunnel;

/// A finished remote agent run.
pub struct AgentRun {
    pub exit_code: i32,
    pub stderr: String,
}

/// How the agent reaches back into xConsole. `None` runs it with only the tools it
/// brings itself, which is what the VPS's own shell already gives it.
pub struct McpBridge {
    /// Loopback port on *this* machine where [`crate::mcp::http`] is serving.
    pub local_port: u16,
    /// Bearer token minted for this run.
    pub token: String,
}

/// PATH additions for a non-interactive SSH command.
///
/// `channel.exec` runs the command in a non-interactive, non-login shell, and every
/// distro's `.bashrc` opens with a guard that returns immediately for exactly that case.
/// So the PATH a person sees when they SSH in by hand is *not* the PATH this gets — and
/// agent CLIs install into precisely the directories that guard skips. The symptom is
/// `claude: command not found` on a server where `which claude` answers fine, which
/// reads as "it is not installed" and is not.
///
/// Prepending rather than replacing, and only where the directory is not already
/// present, so a server that does export a good PATH keeps its own ordering.
const PATH_PRELUDE: &str = r#"for d in "$HOME/.local/bin" "$HOME/bin" "$HOME/.npm-global/bin" "$HOME/.bun/bin" "$HOME/.deno/bin" /usr/local/bin "$HOME"/.nvm/versions/node/*/bin "$HOME"/.volta/bin; do case ":$PATH:" in *":$d:"*) ;; *) [ -d "$d" ] && PATH="$d:$PATH";; esac; done; export PATH; "#;

/// Whether a failure is the shell not finding the binary at all.
///
/// Worth telling apart from every other non-zero exit: it is the one failure the user
/// fixes in the provider settings rather than on the server.
pub fn is_command_not_found(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("command not found") || s.contains("no such file or directory")
}

/// Run one command on `vps_id`, streaming stdout line by line.
///
/// `command_with_mcp` receives the remote loopback URL for the MCP bridge and returns the
/// command line to run. It is a callback because the port is only known after the SSH
/// server has bound the forward — the command cannot be built before then.
pub async fn run_agent(
    db: &Db,
    vps_id: &str,
    bridge: Option<&McpBridge>,
    stdin_data: &str,
    command_with_mcp: impl FnOnce(Option<&str>) -> String,
    cancel: Option<Arc<AtomicBool>>,
    mut on_line: impl FnMut(String),
) -> Result<AgentRun, String> {
    let vps = db
        .get_vps(vps_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{vps_id}' is not configured in xConsole"))?;

    let auth = client::resolve_auth(&vps).map_err(|e| e.to_string())?;
    let connected = client::connect(&vps.host, vps.port, &vps.username, auth, db.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Set the target *before* asking for the forward: the remote side may connect the
    // instant it binds, and a channel arriving with no target set is dropped.
    let mut _reverse = None;
    let mcp_url = match bridge {
        Some(b) => {
            *connected.forward_target.lock().unwrap() = Some(b.local_port);
            // Port 0 asks the server to choose, which avoids colliding with whatever
            // else the user runs on that box.
            let rev = tunnel::open_reverse_forward(&connected.handle, 0).await?;
            let url = format!("http://127.0.0.1:{}/mcp", rev.remote_port);
            _reverse = Some(rev);
            Some(url)
        }
        None => None,
    };

    let command = format!("{PATH_PRELUDE}{}", command_with_mcp(mcp_url.as_deref()));

    let mut channel = connected
        .handle
        .channel_open_session()
        .await
        .map_err(|e| e.to_string())?;
    channel
        .exec(true, command.as_str())
        .await
        .map_err(|e| e.to_string())?;

    if !stdin_data.is_empty() {
        channel
            .data(stdin_data.as_bytes())
            .await
            .map_err(|e| format!("could not send the prompt: {e}"))?;
    }
    // The agent reads until stdin closes; without this it waits forever for more prompt.
    channel.eof().await.map_err(|e| e.to_string())?;

    let mut pending = String::new();
    let mut stderr = String::new();
    let mut exit_code: Option<i32> = None;

    loop {
        if cancel.as_ref().map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
            let _ = channel.close().await;
            break;
        }
        match channel.wait().await {
            Some(ChannelMsg::Data { ref data }) => {
                pending.push_str(&String::from_utf8_lossy(data));
                // stream-json is newline-delimited, and a read can split a line in half
                // or carry several — only emit what is complete.
                while let Some(idx) = pending.find('\n') {
                    let line: String = pending.drain(..=idx).collect();
                    on_line(line.trim_end_matches(['\r', '\n']).to_string());
                }
            }
            Some(ChannelMsg::ExtendedData { ref data, ext }) => {
                if ext == 1 {
                    stderr.push_str(&String::from_utf8_lossy(data));
                } else {
                    pending.push_str(&String::from_utf8_lossy(data));
                }
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status as i32),
            Some(ChannelMsg::Eof) => {
                if exit_code.is_some() {
                    break;
                }
            }
            Some(ChannelMsg::Close) | None => break,
            _ => {}
        }
    }

    // A final line with no trailing newline is still a line.
    if !pending.trim().is_empty() {
        on_line(pending.trim_end_matches(['\r', '\n']).to_string());
    }

    // Dropping the forward and disconnecting closes the remote listener, so the bridge
    // is unreachable the moment the run ends.
    let _ = connected
        .handle
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;

    Ok(AgentRun {
        exit_code: exit_code.unwrap_or(-1),
        stderr,
    })
}

/// The `--mcp-config` value pointing an agent at the tunnelled bridge.
///
/// Built with `serde_json` and shell-quoted, never by string interpolation: the token is
/// about to be pasted into a remote shell command line, and one stray quote in it would
/// either break the run or end the argument early.
pub fn mcp_config_arg(url: &str, token: &str) -> String {
    let cfg = serde_json::json!({
        "mcpServers": {
            "xconsole": {
                "type": "http",
                "url": url,
                "headers": { "Authorization": format!("Bearer {token}") }
            }
        }
    });
    shell_quote(&cfg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mcp_config_is_quoted_as_one_shell_argument() {
        let arg = mcp_config_arg("http://127.0.0.1:40001/mcp", "tok-1");
        assert!(arg.starts_with('\'') && arg.ends_with('\''));
        assert!(arg.contains("\"type\":\"http\""));
        assert!(arg.contains("Bearer tok-1"));
    }

    #[test]
    fn a_quote_in_the_token_cannot_end_the_argument_early() {
        // The token is generated, not user input — but a value that reaches a remote
        // shell command line gets checked anyway, because the day it stops being
        // generated is the day this matters.
        let arg = mcp_config_arg("http://127.0.0.1:1/mcp", "a'b");
        assert!(arg.contains("'\\''"), "single quote must be escaped: {arg}");
        // Exactly one opening and one closing quote survive as delimiters.
        assert!(arg.starts_with('\'') && arg.ends_with('\''));
    }

    #[test]
    fn a_missing_binary_is_told_apart_from_a_failed_run() {
        // It is the only failure fixed in xConsole's settings rather than on the
        // server, so it earns its own message.
        assert!(is_command_not_found("bash: line 1: claude: command not found"));
        assert!(is_command_not_found("sh: 1: claude: not found\nNo such file or directory"));
        assert!(!is_command_not_found("Invalid API key · Run /login"));
        assert!(!is_command_not_found(""));
    }

    #[test]
    fn the_path_prelude_only_prepends_directories_that_exist() {
        // Verified by running it: without this, `claude` installed in ~/.local/bin is
        // not found by an SSH command, which is the bug it exists for. The `-d` guard
        // is what keeps an unmatched glob (`~/.nvm/versions/node/*/bin` on a box with
        // no nvm) from being pushed into PATH as a literal.
        assert!(PATH_PRELUDE.contains("[ -d \"$d\" ]"));
        // Prepend-if-absent, so a server that exports a good PATH keeps its ordering
        // and a repeated run cannot grow PATH without bound.
        assert!(PATH_PRELUDE.contains("case \":$PATH:\" in *\":$d:\"*"));
        assert!(PATH_PRELUDE.contains("export PATH"));
        // Ends with a separator, or it would glue itself onto the command that follows.
        assert!(PATH_PRELUDE.ends_with("; ") || PATH_PRELUDE.ends_with(" "));
        // The directories agent CLIs actually install into.
        for dir in ["$HOME/.local/bin", ".npm-global", ".nvm/versions/node"] {
            assert!(PATH_PRELUDE.contains(dir), "prelude is missing {dir}");
        }
    }
}
