use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::ai::provider::{
    emit, ActivityEvent, ChatRequest, ChatResponse, DiffLine, EventSink, Provider, StreamEvent,
    XConsoleExec,
};
use crate::mcp::prepare_agent_workspace;
use crate::ssh::remote_ops::shell_quote;

/// xConsole session id -> the CLI's own conversation/session id.
///
/// Both Antigravity (`--conversation`) and Claude Code (`--resume`) can continue a
/// thread across invocations, but only if we remember the id they handed back on the
/// first run. Without this every message would start the CLI from scratch.
static CLI_SESSIONS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The CLI's own session id for one xConsole session, when there is one.
///
/// Public so a session can be read back: an agent's report is a claim, and the CLI's
/// transcript is the record it can be checked against.
pub fn get_cli_conversation(session_id: &str) -> Option<String> {
    if session_id.is_empty() {
        return None;
    }
    CLI_SESSIONS.lock().ok()?.get(session_id).cloned()
}

fn store_cli_conversation(session_id: &str, conversation_id: &str) {
    if session_id.is_empty() || conversation_id.is_empty() {
        return;
    }
    if let Ok(mut map) = CLI_SESSIONS.lock() {
        map.insert(session_id.to_string(), conversation_id.to_string());
    }
}

fn remove_cli_conversation(session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    if let Ok(mut map) = CLI_SESSIONS.lock() {
        map.remove(session_id);
    }
}

/// Run the CLI on a server instead of this machine.
///
/// Configured per provider, so "Claude Code on my laptop" and "Claude Code on the build
/// box" are two provider rows and two personas can each pick one.
#[derive(Clone)]
pub struct RemoteTarget {
    pub vps_id: String,
    /// Needed to resolve the server's credentials and open the SSH session.
    pub db: crate::storage::Db,
    /// What the agent may do on that box without asking. Nobody is at a prompt, so this
    /// is decided here rather than deferred to one that would never be answered.
    ///
    /// One of Claude Code's own `--permission-mode` values, verified against
    /// `claude --help`: acceptEdits, auto, bypassPermissions, manual, dontAsk, plan.
    pub permission_mode: String,
    /// The unprivileged account on that box the CLI runs as, when one is configured.
    ///
    /// `None` means it runs as whoever the VPS row logs in as — which for most of these
    /// hosts is root. An agent CLI with a filesystem tool and no supervision does not
    /// need to be root to do its job, and being root is the difference between a bad
    /// edit and an unrecoverable one. Provisioned by `agent_cli_provision`; the row is
    /// only read here, never invented.
    pub run_as_user: Option<String>,
}

pub struct CliProvider {
    kind: String,
    bin: String,
    model: Option<String>,
    api_key: Option<String>,
    remote: Option<RemoteTarget>,
}

/// Whether a failure is the CLI refusing to resume a conversation it no longer has.
///
/// Worth telling apart from every other non-zero exit: the thread is gone whatever
/// happens next, so the only useful response is to start a new one rather than to hand
/// somebody on a phone an error they cannot act on.
fn is_missing_conversation(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("no conversation found")
        || s.contains("session not found")
        || (s.contains("conversation") && s.contains("not found"))
}

impl CliProvider {
    pub fn new(
        kind: String,
        bin: String,
        model: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            kind,
            bin,
            model: model.filter(|s| !s.is_empty()),
            api_key: api_key.filter(|s| !s.is_empty()),
            remote: None,
        }
    }

    /// What Claude Code may do without asking.
    ///
    /// Local runs stay read-only: a CLI on the user's own desktop, driven by a
    /// background persona, editing files nobody asked it to touch is a surprise. A
    /// remote target is configured deliberately for a named server, so it carries
    /// whatever mode that provider row was given.
    fn permission_mode(&self) -> &str {
        match &self.remote {
            Some(r) if !r.permission_mode.trim().is_empty() => r.permission_mode.trim(),
            Some(_) => "acceptEdits",
            None => "dontAsk",
        }
    }

    /// Run the CLI on a server, with xConsole's own tools tunnelled back to it.
    ///
    /// The local path pipes a child process; there is no child here, so this drives an
    /// SSH channel instead and reuses the same stream parser on the far side of it.
    async fn chat_remote(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
        remote: RemoteTarget,
    ) -> Result<ChatResponse, String> {
        if self.kind != "claude_code" {
            return Err(format!(
                "running {} on a server is not supported yet — only Claude Code is",
                self.kind
            ));
        }

        let existing = (!req.session_id.is_empty())
            .then(|| get_cli_conversation(&req.session_id))
            .flatten();
        let prompt = Self::build_prompt(req, existing.is_some());
        let prompt_tokens_est = crate::ai::text::count_tokens(&prompt) as u32;

        emit(
            sink,
            StreamEvent::Status(format!("Starting Claude Code on {}…", remote.vps_id)),
        );

        // Serve xConsole's tools on loopback for the duration of the run. Held in scope:
        // dropping it stops the listener, so the bridge dies with the turn.
        let bridge_server = match &req.xconsole {
            Some(xc) => {
                let session = crate::mcp::server_session_for_bridge(
                    remote.db.clone(),
                    &xc.data_dir,
                    xc.targets.clone(),
                    xc.safety.clone(),
                    xc.workspace_id.clone(),
                );
                Some(crate::mcp::http::serve(session).await?)
            }
            None => None,
        };
        let bridge = bridge_server.as_ref().map(|b| crate::ssh::agent_exec::McpBridge {
            local_port: b.port,
            token: b.token.clone(),
        });

        let bin = shell_quote(&self.remote_bin());
        let flags = self.run_flags(req.xconsole.as_ref(), None, existing.as_deref(), &req.reasoning);
        let model_flags: Vec<String> = flags.iter().map(|f| shell_quote(f)).collect();

        let out_cell = std::sync::Arc::new(std::sync::Mutex::new(ChatResponse::default()));
        let started = std::time::Instant::now();
        let session_id = req.session_id.clone();

        let sink_ref = sink;
        let out_for_lines = out_cell.clone();
        let run = crate::ssh::agent_exec::run_agent(
            &remote.db,
            &remote.vps_id,
            remote.run_as_user.as_deref(),
            bridge.as_ref(),
            &prompt,
            |mcp| {
                let mut cmd = format!("{bin} {}", model_flags.join(" "));
                // Logged before the MCP argument is appended. The config now lives in a
                // mode-600 file on the far side rather than in argv, so there is no
                // token in this line either way — but "what did it actually run" was
                // unanswerable without reproducing the whole path by hand, and the
                // flags are the part that matters.
                crate::diag(&format!("cli(remote): {cmd}"));
                if let Some(h) = mcp {
                    cmd.push_str(" --mcp-config ");
                    cmd.push_str(h.config_path);
                }
                cmd
            },
            req.cancel.clone(),
            |line| {
                let mut out = out_for_lines.lock().unwrap();
                parse_claude_code_stream_line(&line, &session_id, &mut out, sink_ref);
            },
        )
        .await?;

        let mut out = std::sync::Arc::try_unwrap(out_cell)
            .map(|m| m.into_inner().unwrap())
            .unwrap_or_default();

        // A `--output-format stream-json` run always ends with a `result` event, and
        // that event is where the token counts come from. Text arriving without one
        // means the CLI answered in some other format — an older build that does not
        // know the flag, or a launcher printing plain text — and the parser fell back
        // to treating each line as prose. It still shows an answer, so the only visible
        // symptom is a turn that reports no tokens, which reads as a display glitch
        // rather than as "this ran in a mode we do not understand".
        if out.prompt_tokens.is_none() && out.completion_tokens.is_none() {
            crate::diag(&format!(
                "cli(remote): no result event from {} — output was not stream-json, so \
                 token counts and the resume id are missing. stderr: {}",
                remote.vps_id,
                run.stderr.trim().chars().take(400).collect::<String>()
            ));
            emit(
                sink,
                StreamEvent::Status(format!(
                    "Claude Code on {} did not report a result — check its version there \
                     (`claude --version`); xConsole needs one that supports \
                     `--output-format stream-json`.",
                    remote.vps_id
                )),
            );
        }

        // Exit 0 with nothing to show is the case that has been hardest to explain: the
        // run "worked", so no error is raised, and the caller can only report that the
        // agent said nothing. Whatever the far side did print is the only account of
        // why, so it is written down here rather than discarded.
        if run.exit_code == 0 && out.content.trim().is_empty() {
            crate::diag(&format!(
                "cli(remote): {} exited 0 with no answer. stderr: {:?}",
                remote.vps_id,
                run.stderr.trim().chars().take(1000).collect::<String>()
            ));
            emit(
                sink,
                StreamEvent::Status(format!(
                    "Claude Code on {} finished without answering{}",
                    remote.vps_id,
                    match run.stderr.trim() {
                        "" => " and printed nothing — check xconsole.log for the command it ran".to_string(),
                        e => format!(": {}", e.chars().take(200).collect::<String>()),
                    }
                )),
            );
        }

        if run.exit_code != 0 {
            // A failed run's session id is not worth resuming into.
            remove_cli_conversation(&req.session_id);
            if out.content.trim().is_empty() {
                let detail = run.stderr.trim();
                // The conversation this was resuming into is gone — the server was
                // rebuilt, or the CLI expired it. That is not a failure to report to
                // somebody on a phone: the thread is lost either way, and the only
                // useful thing is to answer them. Retried once, fresh, and only once,
                // because a second identical failure is a real problem.
                if is_missing_conversation(detail) && existing.is_some() {
                    crate::diag(&format!(
                        "cli(remote): the conversation on {} is gone; starting a new one",
                        remote.vps_id
                    ));
                    emit(
                        sink,
                        StreamEvent::Status(
                            "The previous conversation is gone on that server — starting a \
                             new one."
                                .into(),
                        ),
                    );
                    let mut fresh = req.clone();
                    // A fresh CLI session has none of the thread, so the whole prompt has
                    // to be rebuilt rather than reduced to the last message.
                    fresh.session_id = String::new();
                    return Box::pin(self.chat_remote(&fresh, sink, remote)).await;
                }
                return Err(if detail.is_empty() {
                    format!(
                        "Claude Code exited with code {} on {}. Check it is installed and \
                         signed in there (`claude setup-token`).",
                        run.exit_code, remote.vps_id
                    )
                } else if crate::ssh::agent_exec::is_command_not_found(detail) {
                    // The one failure the user fixes in xConsole rather than on the
                    // server, and the raw shell error points the wrong way: it reads as
                    // "not installed" on a box where `which claude` answers fine, because
                    // an SSH command runs in a shell that never sourced the profile that
                    // sets PATH.
                    format!(
                        "'{}' was not found on {}. Run `which claude` on that server and \
                         put the full path (e.g. /root/.local/bin/claude) in this \
                         provider's Binary path — an SSH command does not get the PATH \
                         you see when you log in by hand.",
                        self.bin, remote.vps_id
                    )
                } else {
                    format!("Claude Code failed on {}: {detail}", remote.vps_id)
                });
            }
        }

        let completion_tokens = out
            .completion_tokens
            .unwrap_or_else(|| crate::ai::text::count_tokens(&out.content) as u32);
        let prompt_tokens = out.prompt_tokens.unwrap_or(prompt_tokens_est);
        let ms = started.elapsed().as_millis() as u64;
        let secs = (ms as f64 / 1000.0).max(0.05);
        emit(
            sink,
            StreamEvent::Stats(crate::ai::provider::StreamStats {
                completion_tokens,
                prompt_tokens: Some(prompt_tokens),
                cached_tokens: out.cached_tokens,
                cache_creation_tokens: None,
                duration_ms: ms,
                tokens_per_sec: (completion_tokens as f64 / secs) as f32,
            }),
        );
        if out.stop_reason.is_empty() {
            out.stop_reason = "stop".into();
        }
        Ok(out)
    }

    /// Same CLI, running over SSH on `remote.vps_id`.
    pub fn with_remote(mut self, remote: Option<RemoteTarget>) -> Self {
        self.remote = remote;
        self
    }

    /// The command to run on the *far* side of an SSH session.
    ///
    /// [`Self::default_bin`] probes this desktop's filesystem, because on a desktop that
    /// is the only way to find an installer that puts the binary under the user's home
    /// and a GUI app that never sourced the shell's PATH. Shell-quoting the resulting
    /// absolute local path into a remote command is nonsense: `/home/me/.local/bin/claude`
    /// does not exist on the server, and the failure reads as "not installed" on a box
    /// where `which claude` answers fine. So a path that belongs to *this* machine's home
    /// is dropped back to the bare name, and the PATH prelude finds the real one there.
    fn remote_bin(&self) -> String {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok();
        remote_bin_for(&self.kind, &self.bin, home.as_deref())
    }

    pub fn default_bin(kind: &str) -> String {
        match kind {
            "opencode_cli" => "opencode".into(),
            "antigravity_cli" => {
                #[cfg(windows)]
                {
                    if let Ok(local) = std::env::var("LOCALAPPDATA") {
                        let agy = format!(r"{local}\agy\bin\agy.exe");
                        if Path::new(&agy).exists() {
                            return agy;
                        }
                        let ide = format!(r"{local}\Programs\Antigravity IDE\bin\antigravity-ide.cmd");
                        if Path::new(&ide).exists() {
                            return ide;
                        }
                    }
                }
                "agy".into()
            }
            "cursor" => {
                #[cfg(windows)]
                {
                    if let Ok(local) = std::env::var("LOCALAPPDATA") {
                        let cmd = format!(r"{local}\cursor-agent\agent.cmd");
                        if Path::new(&cmd).exists() {
                            return cmd;
                        }
                    }
                }
                "agent".into()
            }
            "grok_cli" => {
                // Same shape as Claude Code: its installer drops the binary under the
                // user's home, and a GUI app never sourced the profile that puts it on
                // PATH. This probe is for *this* desktop only — a remote run starts from
                // `remote_default_bin`, because nothing about this disk describes the
                // server's.
                #[cfg(windows)]
                {
                    if let Ok(profile) = std::env::var("USERPROFILE") {
                        let native = format!(r"{profile}\.grok\bin\grok.exe");
                        if Path::new(&native).exists() {
                            return native;
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    if let Some(home) = std::env::var_os("HOME") {
                        let native = PathBuf::from(home).join(".grok/bin/grok");
                        if native.exists() {
                            return native.to_string_lossy().into_owned();
                        }
                    }
                }
                "grok".into()
            }
            "claude_code" => {
                // The native installer puts it under the user's home rather than on a
                // system path, and a GUI app does not inherit the shell's PATH edits.
                #[cfg(windows)]
                {
                    if let Ok(profile) = std::env::var("USERPROFILE") {
                        let native = format!(r"{profile}\.local\bin\claude.exe");
                        if Path::new(&native).exists() {
                            return native;
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    if let Some(home) = std::env::var_os("HOME") {
                        let native = PathBuf::from(home).join(".local/bin/claude");
                        if native.exists() {
                            return native.to_string_lossy().into_owned();
                        }
                    }
                }
                "claude".into()
            }
            _ => "codex".into(),
        }
    }

    /// CLI flags only — prompt is passed via stdin (avoids Windows cmd length limits).
    fn run_flags(
        &self,
        xconsole: Option<&XConsoleExec>,
        workspace: Option<&Path>,
        agy_conv: Option<&str>,
        reasoning: &str,
    ) -> Vec<String> {
        match self.kind.as_str() {
            "opencode_cli" => {
                let mut a = vec!["run".to_string()];
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
                }
                a
            }
            "antigravity_cli" => {
                // `agy` CLI flags placed BEFORE the prompt argument.
                let mut a = vec![
                    "--dangerously-skip-permissions".to_string(),
                    "--output-format".to_string(),
                    "stream-json".to_string(),
                ];
                if let Some(conv_id) = agy_conv {
                    a.push("--conversation".into());
                    a.push(conv_id.to_string());
                }
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
                }
                if !reasoning.is_empty() && matches!(reasoning, "low" | "medium" | "high") {
                    a.push("--effort".into());
                    a.push(reasoning.to_string());
                }
                a
            }
            "claude_code" => {
                // `claude -p` is the documented non-interactive entry point. Kept off
                // `--bare`: bare mode refuses to read the OAuth credentials, so it would
                // demand an ANTHROPIC_API_KEY from a user whose whole reason for picking
                // this provider is that they have a subscription and no key.
                let mut a = vec![
                    "-p".to_string(),
                    "--output-format".into(),
                    "stream-json".into(),
                    // stream-json refuses to run without it.
                    "--verbose".into(),
                ];
                // Manual is the starting mode for -p, and nobody is watching a prompt
                // here, so one is always passed: the alternative is a run that denies
                // everything by default and reports it as the agent's own conclusion.
                a.push("--permission-mode".into());
                a.push(self.permission_mode().into());
                if let Some(conv_id) = agy_conv {
                    a.push("--resume".into());
                    a.push(conv_id.to_string());
                }
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
                }
                // Claude Code spells the same knob `--effort`, with two levels xConsole
                // does not offer; passing only what it accepts keeps an unknown value
                // from failing the whole run.
                if matches!(reasoning, "low" | "medium" | "high") {
                    a.push("--effort".into());
                    a.push(reasoning.to_string());
                }
                if let Some(ws) = workspace {
                    a.push("--add-dir".into());
                    a.push(ws.to_string_lossy().into_owned());
                }
                a
            }
            "grok_cli" => {
                // Every flag verified against `grok --help` (v1.0.13). The prompt is NOT
                // here: grok's `-p/--single` takes the prompt as its value rather than
                // reading stdin, so it is appended at spawn time next to the text — see
                // `spawn_with_stdin`.
                let mut a = vec!["--output-format".to_string(), "plain".into()];
                // Nobody is at a prompt, so a mode is always passed rather than left at
                // the default that asks and then denies. One of grok's own values:
                // default, acceptEdits, auto, dontAsk, bypassPermissions, plan.
                a.push("--permission-mode".into());
                a.push("dontAsk".into());
                if let Some(conv_id) = agy_conv {
                    a.push("--resume".into());
                    a.push(conv_id.to_string());
                }
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
                }
                // Spelled `--reasoning-effort`, with `--effort` as an alias.
                if matches!(reasoning, "low" | "medium" | "high") {
                    a.push("--reasoning-effort".into());
                    a.push(reasoning.to_string());
                }
                if let Some(ws) = workspace {
                    a.push("--cwd".into());
                    a.push(ws.to_string_lossy().into_owned());
                }
                a
            }
            "cursor" => {
                let mut a = vec![
                    "-p".into(),
                    "--trust".into(),
                    "--force".into(),
                    "--approve-mcps".into(),
                ];
                if xconsole.is_some() {
                    a.push("--output-format".into());
                    a.push("stream-json".into());
                    a.push("--stream-partial-output".into());
                    if let Some(ws) = workspace {
                        a.push("--workspace".into());
                        a.push(ws.to_string_lossy().into_owned());
                    }
                } else {
                    a.push("--output-format".into());
                    a.push("text".into());
                }
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
                }
                a
            }
            _ => {
                let mut a = vec!["exec".to_string()];
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
                }
                a
            }
        }
    }

    /// The text handed to the CLI on stdin.
    ///
    /// A resumed conversation only needs what is new, because the CLI still holds the
    /// thread on its side. What is new is the user's message — **not** whatever message
    /// happens to be last.
    ///
    /// Every request ends with a synthetic `# Runtime context` user message carrying the
    /// date, the VPS snapshot and the canvas (see `context::inject_dynamic_into_last_user`).
    /// Taking the last user message therefore sent the model a block of context and
    /// nothing the user had typed. It answered the only thing it had been given: it
    /// narrated the snapshot, or — with nothing worth narrating — said "I'm here, tell me
    /// what you want to do". Asking "why is the disk full?" produced a diff against the
    /// previous snapshot; asking it to delete something produced an offer to help. The
    /// replies looked like a model ignoring the conversation because it never saw it.
    ///
    /// So: the real message, with the runtime context ahead of it as context rather than
    /// in place of it.
    fn build_prompt(req: &ChatRequest, is_resumed_agy: bool) -> String {
        let is_runtime = crate::ai::context::is_runtime_message;
        if is_resumed_agy {
            if let Some(last_user) = req
                .messages
                .iter()
                .rev()
                .find(|m| m.role == "user" && !is_runtime(m))
            {
                let runtime = req
                    .messages
                    .iter()
                    .rev()
                    .find(|m| is_runtime(m))
                    .map(|m| m.content.as_str())
                    .unwrap_or("");
                return if runtime.is_empty() {
                    last_user.content.clone()
                } else {
                    // The ask goes last so it is the thing being answered, not the
                    // thing buried under a screenful of context.
                    format!("{runtime}\n\n{}", last_user.content)
                };
            }
        }
        let mut s = String::new();
        if !req.system.is_empty() {
            s.push_str(&req.system);
            s.push_str("\n\n");
        }
        for m in &req.messages {
            match m.role.as_str() {
                // The runtime block is context, not something the user said. Labelling
                // it "User:" invites the model to answer it.
                "user" if is_runtime(m) => {
                    s.push_str(&m.content);
                    s.push('\n');
                }
                "user" => {
                    s.push_str("User: ");
                    s.push_str(&m.content);
                    s.push('\n');
                }
                "assistant" if !m.content.is_empty() => {
                    s.push_str("Assistant: ");
                    s.push_str(&m.content);
                    s.push('\n');
                }
                _ => {}
            }
        }
        s
    }

    /// For Cursor on Windows: invoke bundled node.exe + index.js directly so we can
    /// pipe stdin without `cmd /C` (which breaks piping and hits argv limits).
    fn resolve_cursor_runtime() -> Option<(PathBuf, PathBuf)> {
        let local = std::env::var("LOCALAPPDATA").ok()?;
        let versions = PathBuf::from(local).join("cursor-agent").join("versions");
        let mut best: Option<((i32, i32, i32), PathBuf)> = None;
        if let Ok(entries) = std::fs::read_dir(&versions) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("20") {
                    continue;
                }
                let date_part = name.split('-').next().unwrap_or("");
                let parts: Vec<&str> = date_part.split('.').collect();
                if parts.len() != 3 {
                    continue;
                }
                // Compare as a numeric (year, minor, patch) tuple so 2024.10.2
                // correctly outranks 2024.1.15 (string-concat would not).
                let key = (
                    parts[0].parse::<i32>().unwrap_or(0),
                    parts[1].parse::<i32>().unwrap_or(0),
                    parts[2].parse::<i32>().unwrap_or(0),
                );
                let node = path.join("node.exe");
                let index = path.join("index.js");
                if node.exists() && index.exists() {
                    if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
                        best = Some((key, path));
                    }
                }
            }
        }
        best.map(|(_, dir)| (dir.join("node.exe"), dir.join("index.js")))
    }
}

/// Build the base command to run the Cursor agent. Prefers the bundled
/// `node.exe index.js` (a real executable that accepts stdin); falls back to the
/// `agent.cmd` launcher via `cmd /C` (Windows can't CreateProcess a `.cmd`
/// directly, which is the "program not found" people hit when launching `agent`).
fn cursor_base_command(bin: &str) -> Command {
    if let Some((node, index)) = CliProvider::resolve_cursor_runtime() {
        let mut c = crate::proc::quiet_tokio(node);
        c.arg(index);
        return c;
    }
    #[cfg(windows)]
    {
        // A configured/installed .cmd or .ps1 launcher → run it through its host.
        let path = std::path::Path::new(bin);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if bin.contains('\\') || bin.contains('/') {
            if ext == "cmd" || ext == "bat" {
                let mut c = crate::proc::quiet_tokio("cmd");
                c.arg("/C").arg(bin);
                return c;
            }
            if ext == "ps1" {
                let mut c = crate::proc::quiet_tokio("powershell");
                c.arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass").arg("-File").arg(bin);
                return c;
            }
        }
        // Bare name like "agent": resolve the known install location's .cmd.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let cmd_path = format!(r"{local}\cursor-agent\agent.cmd");
            if std::path::Path::new(&cmd_path).exists() {
                let mut c = crate::proc::quiet_tokio("cmd");
                c.arg("/C").arg(cmd_path);
                return c;
            }
        }
    }
    crate::proc::quiet_tokio(bin)
}

/// Spawn a CLI process. Prompt is written to stdin when `prompt` is Some.
async fn spawn_with_stdin(
    kind: &str,
    bin: &str,
    flags: &[String],
    prompt: &str,
    api_key: Option<&str>,
    workspace: Option<&Path>,
) -> Result<tokio::process::Child, String> {
    let mut cmd = if kind == "cursor" {
        cursor_base_command(bin)
    } else {
        spawn_cli_program(bin)?
    };

    if let Some(ws) = workspace {
        cmd.current_dir(ws);
    }

    cmd.args(flags);
    // Grok is the odd one out: `-p/--single <PROMPT>` takes the prompt as the flag's
    // value, where every other CLI here reads it from stdin. Piping to it would hang
    // waiting for input that is never asked for, so the text goes in argv and stdin is
    // closed immediately. Placed last because clap accepts flags in any order.
    let prompt_in_argv = kind == "grok_cli";
    if prompt_in_argv {
        cmd.arg("-p").arg(prompt);
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if kind == "cursor" {
        if let Some(key) = api_key {
            cmd.env("CURSOR_API_KEY", key);
        }
    }
    // Optional on purpose. With no key set, Claude Code falls back to the machine's own
    // subscription login — which is the whole reason to run the CLI instead of the API.
    if kind == "claude_code" {
        if let Some(key) = api_key {
            cmd.env("ANTHROPIC_API_KEY", key);
        }
    }

    crate::proc::hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        if kind == "cursor" {
            format!(
                "failed to launch Cursor agent: {e}. Install from https://cursor.com/docs/cli"
            )
        } else {
            format!("failed to launch '{bin}': {e}")
        }
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        if !prompt_in_argv {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| format!("failed to write prompt to CLI stdin: {e}"))?;
        }
        // Dropped either way: the CLI reads until stdin closes, and one that was given
        // its prompt in argv still waits for the close.
        drop(stdin);
    }

    Ok(child)
}

fn spawn_cli_program(bin: &str) -> Result<Command, String> {
    #[cfg(windows)]
    {
        let lower = bin.to_ascii_lowercase();
        // Explicit .cmd / .bat path → run through cmd /C.
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            let mut cmd = crate::proc::quiet_tokio("cmd");
            cmd.arg("/C").arg(bin);
            return Ok(cmd);
        }
        if !Path::new(bin).exists() {
            if lower == "agy" || lower == "antigravity" {
                let def = CliProvider::default_bin("antigravity_cli");
                if Path::new(&def).exists() {
                    return Ok(crate::proc::quiet_tokio(def));
                }
            }
            if !bin.contains('\\') && !bin.contains('/') && !bin.contains('.') {
                if let Some(resolved) = resolve_cmd_on_path(bin) {
                    let mut cmd = crate::proc::quiet_tokio("cmd");
                    cmd.arg("/C").arg(resolved);
                    return Ok(cmd);
                }
            }
        }
    }
    Ok(crate::proc::quiet_tokio(bin))
}

/// Search PATH for `name.cmd` or `name.bat` and return the first hit.
#[cfg(windows)]
fn resolve_cmd_on_path(name: &str) -> Option<String> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        for ext in &["cmd", "bat"] {
            let candidate = dir.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn spawn_cli(bin: &str, args: &[String]) -> Result<Command, String> {
    let mut cmd = spawn_cli_program(bin)?;
    cmd.args(args);
    Ok(cmd)
}

async fn read_child_output(
    mut child: tokio::process::Child,
    bin: &str,
    kind: &str,
    session_id: &str,
    sink: Option<&EventSink>,
    stream_json: bool,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<ChatResponse, String> {
    use std::sync::atomic::Ordering;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Drain stdout and stderr concurrently *before* waiting: if the child writes
    // more than a pipe buffer (~64KB) to stderr while we're blocked on stdout
    // (or vice versa) a sequential reader deadlocks.
    let cancel_out = cancel.clone();
    let session_id_owned = session_id.to_string();
    let kind_owned = kind.to_string();
    let stdout_fut = async move {
        let mut out = ChatResponse::default();
        if let Some(stdout) = stdout {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                tokio::select! {
                    next = lines.next_line() => match next {
                        Ok(Some(line)) => {
                            if stream_json {
                                match kind_owned.as_str() {
                                    "antigravity_cli" => parse_antigravity_stream_line(&line, &session_id_owned, &mut out, sink),
                                    "claude_code" => parse_claude_code_stream_line(&line, &session_id_owned, &mut out, sink),
                                    _ => parse_cursor_stream_line(&line, &mut out, sink),
                                }
                            } else {
                                out.content.push_str(&line);
                                out.content.push('\n');
                                emit(sink, StreamEvent::Text(format!("{line}\n")));
                            }
                        }
                        _ => break,
                    },
                    // Wake periodically so Stop is honored even while the child is quiet.
                    _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
                }
                if cancel_out.as_ref().map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                    emit(sink, StreamEvent::Status("Stopped.".into()));
                    break;
                }
            }
        }
        out
    };
    let cancel_err = cancel.clone();
    let stderr_fut = async move {
        let mut err = String::new();
        if let Some(stderr) = stderr {
            let mut lines = BufReader::new(stderr).lines();
            loop {
                tokio::select! {
                    next = lines.next_line() => match next {
                        Ok(Some(line)) => {
                            err.push_str(&line);
                            err.push('\n');
                        }
                        _ => break,
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
                }
                if cancel_err.as_ref().map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                    break;
                }
            }
        }
        err
    };
    let (mut out, err) = tokio::join!(stdout_fut, stderr_fut);

    // Stop pressed mid-run: kill the agent process and return what we have.
    if cancel.as_ref().map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
        let _ = child.start_kill();
        let _ = child.wait().await;
        out.stop_reason = "stop".into();
        return Ok(out);
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;
    if !status.success() {
        if matches!(kind, "antigravity_cli" | "claude_code") {
            remove_cli_conversation(session_id);
        }
        let hint = if kind == "cursor"
            && (err.contains("invalid") || err.contains("Not logged in") || err.contains("API key"))
        {
            " Check your API key or use Login in Settings → Providers."
        } else {
            ""
        };
        let err_detail = err.trim();
        let msg = if err_detail.is_empty() {
            format!(
                "{} exited with status code {}{}",
                bin,
                status.code().unwrap_or(-1),
                hint
            )
        } else {
            format!(
                "{} exited with {}: {}{}",
                bin,
                status.code().unwrap_or(-1),
                err_detail,
                hint
            )
        };
        return Err(msg);
    }

    out.stop_reason = "stop".into();
    Ok(out)
}

fn format_agy_tool_label(
    name: &str,
    params: Option<&serde_json::Value>,
) -> (String, Option<String>) {
    let stripped = strip_mcp_server_prefix(name);
    match stripped {
        "run_command" | "bash" | "shell" => {
            let cmd = params
                .and_then(|p| {
                    p.get("command")
                        .or_else(|| p.get("CommandLine"))
                        .or_else(|| p.get("cmd"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            let is_mcp = params.and_then(|p| p.get("command")).is_some()
                || name.contains("xconsole")
                || name.contains("mcp");
            let prefix = if is_mcp { "SSH" } else { "Shell" };
            (
                format!("{prefix} › {}", truncate_str(cmd, 72)),
                Some(cmd.to_string()),
            )
        }
        "read_file" => {
            let path = params
                .and_then(|p| {
                    p.get("path")
                        .or_else(|| p.get("AbsolutePath"))
                        .or_else(|| p.get("file"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Read file · {path}"), None)
        }
        "write_file" => {
            let path = params
                .and_then(|p| {
                    p.get("path")
                        .or_else(|| p.get("TargetFile"))
                        .or_else(|| p.get("file"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            let content = params
                .and_then(|p| p.get("content").or_else(|| p.get("CodeContent")))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            (
                format!("Write file · {path}"),
                if content.is_empty() {
                    None
                } else {
                    Some(content.to_string())
                },
            )
        }
        "list_vps_targets" => ("List VPS targets".into(), None),
        "canvas_open_terminal" => ("Open Terminal".into(), None),
        "canvas_open_sftp" => ("Open SFTP".into(), None),
        "canvas_tile" => ("Tile canvas".into(), None),
        "canvas_close" => ("Close canvas".into(), None),
        "list_dir" => {
            let path = params
                .and_then(|p| {
                    p.get("DirectoryPath")
                        .or_else(|| p.get("path"))
                        .or_else(|| p.get("dir"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("List directory · {path}"), None)
        }
        "view_file" | "read_resource" => {
            let path = params
                .and_then(|p| {
                    p.get("AbsolutePath")
                        .or_else(|| p.get("path"))
                        .or_else(|| p.get("file"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Read file · {path}"), None)
        }
        "write_to_file" | "replace_file_content" | "multi_replace_file_content" | "edit_file" => {
            let path = params
                .and_then(|p| {
                    p.get("TargetFile")
                        .or_else(|| p.get("path"))
                        .or_else(|| p.get("file"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Write file · {path}"), None)
        }
        "grep_search" | "grep" => {
            let query = params
                .and_then(|p| {
                    p.get("Query")
                        .or_else(|| p.get("query"))
                        .or_else(|| p.get("pattern"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Search · {query}"), None)
        }
        "search_web" | "web_search" => {
            let query = params
                .and_then(|p| p.get("query").or_else(|| p.get("Query")))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Web search · {query}"), None)
        }
        "read_url_content" | "web_fetch" => {
            let url = params
                .and_then(|p| p.get("Url").or_else(|| p.get("url")))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Web fetch · {url}"), None)
        }
        other => (other.replace('_', " "), None),
    }
}

fn parse_antigravity_stream_line(
    line: &str,
    session_id: &str,
    out: &mut ChatResponse,
    sink: Option<&EventSink>,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        out.content.push_str(line);
        out.content.push('\n');
        emit(sink, StreamEvent::Text(format!("{line}\n")));
        return;
    };

    let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");
    match event {
        "init" => {
            if let Some(conv_id) = v.get("conversation_id").and_then(|c| c.as_str()) {
                if !conv_id.is_empty() && !session_id.is_empty() {
                    store_cli_conversation(session_id, conv_id);
                }
            }
        }
        "step_update" => {
            let Some(su) = v.get("step_update") else {
                return;
            };
            if let Some(conv_id) = su.get("conversation_id").and_then(|c| c.as_str()) {
                if !conv_id.is_empty() && !session_id.is_empty() {
                    store_cli_conversation(session_id, conv_id);
                }
            }
            let step_type = su.get("step_type").and_then(|s| s.as_str()).unwrap_or("");
            let state = su.get("state").and_then(|s| s.as_str()).unwrap_or("");
            let step_idx = su.get("step_index").and_then(|i| i.as_u64()).unwrap_or(0);
            let call_id = format!("agy-step-{step_idx}");

            if step_type == "agent_response" {
                if let Some(delta) = su.get("text_delta").and_then(|d| d.as_str()) {
                    if !delta.is_empty() {
                        out.content.push_str(delta);
                        emit(sink, StreamEvent::Text(delta.to_string()));
                    }
                }
            } else if step_type == "tool" {
                let tool_name = su
                    .get("tool_name")
                    .and_then(|t| t.as_str())
                    .or_else(|| su.pointer("/tool_info/name").and_then(|t| t.as_str()))
                    .unwrap_or("tool");
                if state == "ACTIVE" {
                    let params = su.pointer("/tool_info/parameters");
                    let (label, detail) = format_agy_tool_label(tool_name, params);
                    emit(
                        sink,
                        StreamEvent::Activity(ActivityEvent::ToolStart {
                            id: call_id,
                            tool: tool_name.to_string(),
                            label,
                            detail,
                        }),
                    );
                } else if state == "DONE" {
                    emit(
                        sink,
                        StreamEvent::Activity(ActivityEvent::ToolEnd {
                            id: call_id,
                            ok: true,
                        }),
                    );
                } else if state == "ERROR" {
                    emit(
                        sink,
                        StreamEvent::Activity(ActivityEvent::ToolEnd {
                            id: call_id,
                            ok: false,
                        }),
                    );
                }
            }

            if let Some(u) = su.get("usage") {
                if let Some(inp) = u.get("input_tokens").and_then(|v| v.as_u64()) {
                    out.prompt_tokens = Some(inp as u32);
                }
                if let Some(outp) = u.get("output_tokens").and_then(|v| v.as_u64()) {
                    out.completion_tokens = Some(outp as u32);
                }
                if let Some(cached) = u.get("cache_read_tokens").and_then(|v| v.as_u64()) {
                    out.cached_tokens = Some(cached as u32);
                }
            }
        }
        "result" => {
            let Some(res) = v.get("result") else {
                return;
            };
            if let Some(conv_id) = res.get("conversation_id").and_then(|c| c.as_str()) {
                if !conv_id.is_empty() && !session_id.is_empty() {
                    store_cli_conversation(session_id, conv_id);
                }
            }
            if let Some(resp_text) = res.get("response").and_then(|r| r.as_str()) {
                if out.content.is_empty() && !resp_text.is_empty() {
                    out.content = resp_text.to_string();
                }
            }
            if let Some(u) = res.get("usage") {
                if let Some(inp) = u.get("input_tokens").and_then(|v| v.as_u64()) {
                    out.prompt_tokens = Some(inp as u32);
                }
                if let Some(outp) = u.get("output_tokens").and_then(|v| v.as_u64()) {
                    out.completion_tokens = Some(outp as u32);
                }
                if let Some(cached) = u.get("cache_read_tokens").and_then(|v| v.as_u64()) {
                    out.cached_tokens = Some(cached as u32);
                }
            }
        }
        _ => {}
    }
}

fn parse_cursor_stream_line(line: &str, out: &mut ChatResponse, sink: Option<&EventSink>) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        out.content.push_str(line);
        out.content.push('\n');
        emit(sink, StreamEvent::Text(format!("{line}\n")));
        return;
    };

    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match ty {
        "assistant" => {
            // Skip duplicate flushes (see Cursor stream-json docs).
            if v.get("model_call_id").is_some() {
                return;
            }
            if let Some(text) = assistant_text(&v) {
                if v.get("timestamp_ms").is_some() {
                    emit(sink, StreamEvent::Text(text.clone()));
                    out.content.push_str(&text);
                } else if out.content.is_empty() || !out.content.ends_with(&text) {
                    emit(sink, StreamEvent::Text(text.clone()));
                    out.content = text;
                }
            }
        }
        "tool_call" => {
            let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
            let call_id = v
                .get("call_id")
                .and_then(|c| c.as_str())
                .unwrap_or("tool")
                .to_string();
            if subtype == "started" {
                let (name, label, detail) = cursor_tool_label(&v);
                if cursor_tool_is_noise(&name, &label) {
                    return;
                }
                emit(
                    sink,
                    StreamEvent::Activity(ActivityEvent::ToolStart {
                        id: call_id.clone(),
                        tool: name.clone(),
                        label,
                        detail,
                    }),
                );
            } else if subtype == "completed" {
                let output = cursor_tool_output(&v);
                emit(
                    sink,
                    StreamEvent::ToolResult {
                        id: call_id.clone(),
                        output: output.clone(),
                    },
                );
                let ok = !output.starts_with("error");
                if let Some(edit) = cursor_file_edit(&v, &call_id) {
                    emit(sink, StreamEvent::Activity(edit));
                }
                emit(
                    sink,
                    StreamEvent::Activity(ActivityEvent::ToolEnd {
                        id: call_id,
                        ok,
                    }),
                );
            }
        }
        "result" => {
            if let Some(text) = v.get("result").and_then(|r| r.as_str()) {
                if !text.is_empty() {
                    out.content = text.to_string();
                }
            }
        }
        _ => {}
    }
}

/// Claude Code's `--output-format stream-json`.
///
/// Deliberately not the Cursor parser: that one *replaces* accumulated content on each
/// assistant message, which is right for Cursor's repeated flushes of one growing
/// answer and wrong here. Claude Code emits a separate assistant message per model
/// response, so a turn that stops to call a tool and then keeps talking arrives as two —
/// replacing would silently drop everything it said before the first tool call.
pub(crate) fn parse_claude_code_stream_line(
    line: &str,
    session_id: &str,
    out: &mut ChatResponse,
    sink: Option<&EventSink>,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        // Not JSON: almost always a launcher warning on stdout. Surfacing it beats
        // swallowing the one line that explains why the run produced nothing.
        emit(sink, StreamEvent::Text(format!("{line}\n")));
        out.content.push_str(line);
        out.content.push('\n');
        return;
    };

    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "system" => {
            if v.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                // Remembered so the next message in this chat resumes the thread rather
                // than re-explaining itself to a fresh session.
                if let Some(id) = v.get("session_id").and_then(|c| c.as_str()) {
                    store_cli_conversation(session_id, id);
                }
                if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
                    emit(sink, StreamEvent::Status(format!("Claude Code on {model}")));
                }
                // Whether xConsole's own tools actually attached.
                //
                // A server the CLI could not reach is reported here and then simply not
                // offered — the run continues, tool-less, and the agent explains to the
                // user that it cannot touch their servers and hands them shell commands
                // to paste. Nothing in xConsole said the bridge had failed, so that read
                // as the agent being unhelpful rather than as a broken tunnel.
                let failed: Vec<String> = v
                    .get("mcp_servers")
                    .and_then(|m| m.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter(|s| {
                                s.get("status").and_then(|st| st.as_str()).unwrap_or("") != "connected"
                            })
                            .map(|s| {
                                format!(
                                    "{} ({})",
                                    s.get("name").and_then(|n| n.as_str()).unwrap_or("?"),
                                    s.get("status").and_then(|st| st.as_str()).unwrap_or("unknown")
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !failed.is_empty() {
                    let msg = format!(
                        "xConsole's tools did not attach to Claude Code: {}. It will answer \
                         from the snapshot but cannot run anything on your servers this turn.",
                        failed.join(", ")
                    );
                    crate::diag(&format!("cli: {msg}"));
                    emit(sink, StreamEvent::Status(msg));
                }
            }
        }
        "assistant" => {
            // Subagent chatter carries the spawning tool call's id; only the main
            // conversation's text is this turn's answer.
            if v.get("parent_tool_use_id").map(|p| !p.is_null()).unwrap_or(false) {
                return;
            }
            for block in v
                .pointer("/message/content")
                .and_then(|c| c.as_array())
                .into_iter()
                .flatten()
            {
                match block.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "text" => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            if !t.is_empty() {
                                emit(sink, StreamEvent::Text(t.to_string()));
                                out.content.push_str(t);
                            }
                        }
                    }
                    "tool_use" => {
                        let id = block
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        let tool = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        emit(
                            sink,
                            StreamEvent::Activity(ActivityEvent::ToolStart {
                                id,
                                label: tool.clone(),
                                detail: claude_code_tool_detail(block),
                                tool,
                            }),
                        );
                    }
                    _ => {}
                }
            }
        }
        // Tool results come back addressed to the agent as a `user` message.
        "user" => {
            for block in v
                .pointer("/message/content")
                .and_then(|c| c.as_array())
                .into_iter()
                .flatten()
            {
                if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                    continue;
                }
                let id = block
                    .get("tool_use_id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let ok = !block.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
                emit(
                    sink,
                    StreamEvent::ToolResult {
                        id: id.clone(),
                        output: claude_code_result_text(block),
                    },
                );
                emit(sink, StreamEvent::Activity(ActivityEvent::ToolEnd { id, ok }));
            }
        }
        "result" => {
            if let Some(u) = v.get("usage") {
                let n = |k: &str| u.get(k).and_then(|x| x.as_u64()).map(|x| x as u32);
                out.prompt_tokens = n("input_tokens");
                out.completion_tokens = n("output_tokens");
                out.cached_tokens = n("cache_read_input_tokens");
            }
            let final_text = v.get("result").and_then(|r| r.as_str()).unwrap_or("");
            // The result line repeats the final answer. Take it only when the assistant
            // messages gave us nothing — appending it to text we already streamed would
            // show the user the same paragraph twice.
            if out.content.trim().is_empty() && !final_text.is_empty() {
                out.content = final_text.to_string();
                emit(sink, StreamEvent::Text(final_text.to_string()));
            }
            if v.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                out.stop_reason = "error".into();
            } else if let Some(reason) = v.get("stop_reason").and_then(|r| r.as_str()) {
                out.stop_reason = reason.to_string();
            }
        }
        _ => {}
    }
}

/// A one-line "what is it doing" for a tool call, for the activity row.
fn claude_code_tool_detail(block: &serde_json::Value) -> Option<String> {
    let input = block.get("input")?;
    for key in ["command", "file_path", "path", "pattern", "url", "description"] {
        if let Some(v) = input.get(key).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                return Some(truncate_str(v, 160));
            }
        }
    }
    None
}

/// Tool-result content is either a plain string or a list of content blocks.
fn claude_code_result_text(block: &serde_json::Value) -> String {
    match block.get("content") {
        Some(serde_json::Value::String(s)) => truncate_str(s, 4000),
        Some(serde_json::Value::Array(parts)) => {
            let joined: String = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("");
            truncate_str(&joined, 4000)
        }
        _ => String::new(),
    }
}

fn assistant_text(v: &serde_json::Value) -> Option<String> {
    let parts: Vec<&str> = v
        .get("message")?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(""))
    }
}

fn cursor_tool_is_noise(name: &str, label: &str) -> bool {
    name.contains("listMcp")
        || name.contains("readMcp")
        || label.contains("listMcp")
        || label.eq_ignore_ascii_case("mcp")
}

fn truncate_str(s: &str, max: usize) -> String {
    // Operate on chars, never byte offsets, so multibyte tool args (accented
    // paths, box-drawing output) can't panic mid-codepoint.
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

fn parse_json_args(raw: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let v = raw?;
    if v.is_object() {
        return Some(v.clone());
    }
    v.as_str()
        .and_then(|s| serde_json::from_str(s).ok())
}

fn human_mcp_label(tool: &str, params: Option<&serde_json::Value>) -> (String, Option<String>) {
    match tool {
        "run_command" => {
            let cmd = params
                .and_then(|p| p.get("command"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (
                format!("SSH › {}", truncate_str(cmd, 72)),
                Some(cmd.to_string()),
            )
        }
        "read_file" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Read file · {path}"), None)
        }
        "read_file_range" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            let offset = params.and_then(|p| p.get("offset")).and_then(|c| c.as_u64()).unwrap_or(1);
            let limit = params.and_then(|p| p.get("limit")).and_then(|c| c.as_u64()).unwrap_or(250);
            (format!("Read {path} (lines {offset}–{})", offset + limit - 1), None)
        }
        "edit_file" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Edit file · {path}"), None)
        }
        "write_file" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            let content = params
                .and_then(|p| p.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            (
                format!("Write file · {path}"),
                if content.is_empty() {
                    None
                } else {
                    Some(content.to_string())
                },
            )
        }
        "list_directory" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|c| c.as_str())
                .unwrap_or(".");
            (format!("List directory · {path}"), None)
        }
        "grep_search" => {
            let pat = params
                .and_then(|p| p.get("pattern"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|c| c.as_str())
                .unwrap_or(".");
            (format!("Grep '{pat}' in {path}"), None)
        }
        "file_search" => {
            let pat = params
                .and_then(|p| p.get("pattern"))
                .and_then(|c| c.as_str())
                .unwrap_or("…");
            (format!("Find files '{pat}'"), None)
        }
        "list_vps_targets" => ("List VPS targets".into(), None),
        "skills_list" => ("List skills".into(), None),
        "skill_view" => {
            let cat = params
                .and_then(|p| p.get("category"))
                .and_then(|c| c.as_str())
                .unwrap_or("?");
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(|c| c.as_str())
                .unwrap_or("?");
            (format!("Read skill · {cat}/{name}"), None)
        }
        "skill_save" => {
            let cat = params
                .and_then(|p| p.get("category"))
                .and_then(|c| c.as_str())
                .unwrap_or("?");
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(|c| c.as_str())
                .unwrap_or("?");
            (format!("Save skill · {cat}/{name}"), None)
        }
        "memory_save" => ("Save to memory".into(), None),
        other => (other.replace('_', " "), None),
    }
}

fn strip_mcp_server_prefix(name: &str) -> &str {
    name.trim()
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .trim_start_matches("xconsole-")
        .trim_start_matches("xconsole_")
        .trim_start_matches("mcp_")
}

fn label_mcp_tool_call(mcp: &serde_json::Value) -> (String, String, Option<String>) {
    let args_wrap = mcp.get("args").unwrap_or(mcp);
    let raw_name = args_wrap
        .get("tool_name")
        .or(args_wrap.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("run_command");
    let tool_name = strip_mcp_server_prefix(raw_name);
    let inner = args_wrap.get("args");
    let (label, detail) = human_mcp_label(tool_name, inner);
    (tool_name.to_string(), label, detail)
}

fn cursor_tool_label(v: &serde_json::Value) -> (String, String, Option<String>) {
    let tool_call = v.get("tool_call").cloned().unwrap_or_default();

    if let Some(f) = tool_call.get("function") {
        let raw = f
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("tool");
        let name = strip_mcp_server_prefix(raw);
        let params = parse_json_args(f.get("arguments"));
        let (label, detail) = human_mcp_label(name, params.as_ref());
        return (name.to_string(), label, detail);
    }

    if let Some(mcp) = tool_call.get("mcpToolCall") {
        return label_mcp_tool_call(mcp);
    }

    if let Some(obj) = tool_call.as_object() {
        for (key, val) in obj {
            if key.ends_with("ToolCall") && key != "listMcpResourcesToolCall" && key != "readMcpResourceToolCall" {
                if let Some(mcp) = val.get("mcpToolCall") {
                    return label_mcp_tool_call(mcp);
                }
                if key == "mcpToolCall" || key.starts_with("xconsole") {
                    return label_mcp_tool_call(val);
                }
            }
            match key.as_str() {
                "shellToolCall" | "bashToolCall" | "runTerminalCmdToolCall" => {
                    let cmd = val
                        .pointer("/args/command")
                        .or_else(|| val.pointer("/args/cmd"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("…");
                    return (
                        "shell".into(),
                        format!("Shell › {}", truncate_str(cmd, 72)),
                        Some(cmd.to_string()),
                    );
                }
                "readToolCall" => {
                    let path = val
                        .pointer("/args/path")
                        .and_then(|p| p.as_str())
                        .unwrap_or("…");
                    return ("read".into(), format!("Read file · {path}"), None);
                }
                "writeToolCall" => {
                    let path = val
                        .pointer("/args/path")
                        .and_then(|p| p.as_str())
                        .unwrap_or("…");
                    let content = val
                        .pointer("/args/fileText")
                        .or_else(|| val.pointer("/args/content"))
                        .or_else(|| val.pointer("/args/streamContent"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    return (
                        "write".into(),
                        format!("Write file · {path}"),
                        if content.is_empty() {
                            None
                        } else {
                            Some(content.to_string())
                        },
                    );
                }
                "searchReplaceToolCall" | "editToolCall" | "editFileToolCall" => {
                    let path = val
                        .pointer("/args/path")
                        .and_then(|p| p.as_str())
                        .unwrap_or("…");
                    return ("edit".into(), format!("Write file · {path}"), None);
                }
                "grepToolCall" => {
                    let pattern = val
                        .pointer("/args/pattern")
                        .and_then(|p| p.as_str())
                        .unwrap_or("…");
                    return ("grep".into(), format!("Search · {pattern}"), None);
                }
                "listMcpResourcesToolCall" | "readMcpResourceToolCall" => {
                    return ("mcp-probe".into(), String::new(), None);
                }
                "mcpToolCall" => return label_mcp_tool_call(val),
                _ => {}
            }
        }
    }

    ("tool".into(), "Working…".into(), None)
}

fn cursor_tool_output(v: &serde_json::Value) -> String {
    let tool_call = v.get("tool_call").cloned().unwrap_or_default();
    if let Some(result) = tool_call.pointer("/function/result") {
        return result.to_string();
    }
    if let Some(mcp) = tool_call.get("mcpToolCall") {
        return mcp_result_text(mcp);
    }
    if let Some(obj) = tool_call.as_object() {
        for val in obj.values() {
            if val.get("mcpToolCall").is_some() {
                return mcp_result_text(val);
            }
            if let Some(text) = extract_tool_result_text(val) {
                return text;
            }
        }
    }
    tool_call.to_string()
}

fn mcp_result_text(mcp: &serde_json::Value) -> String {
    if let Some(text) = extract_tool_result_text(mcp) {
        return text;
    }
    mcp.to_string()
}

fn extract_tool_result_text(val: &serde_json::Value) -> Option<String> {
    if let Some(success) = val.pointer("/result/success") {
        if let Some(content) = success.get("content") {
            if let Some(text) = content.as_str() {
                return Some(text.to_string());
            }
            if let Some(arr) = content.as_array() {
                let parts: Vec<String> = arr
                    .iter()
                    .filter_map(|item| {
                        item.get("text")
                            .and_then(|t| t.get("text").or(Some(t)))
                            .and_then(|t| t.as_str())
                            .map(String::from)
                            .or_else(|| item.as_str().map(String::from))
                    })
                    .collect();
                if !parts.is_empty() {
                    return Some(parts.join("\n"));
                }
            }
        }
        if let Some(content) = success.get("content").and_then(|c| c.as_str()) {
            return Some(content.to_string());
        }
    }
    if let Some(content) = val.pointer("/result/content") {
        return Some(content.to_string());
    }
    None
}

const MAX_DIFF_LINES: usize = 28;

fn file_basename(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn compute_replace_diff(old: &str, new: &str) -> (usize, usize, Vec<DiffLine>) {
    let mut hunks = Vec::new();
    let removed = old.lines().count();
    let added = new.lines().count();
    for line in old.lines() {
        if hunks.len() >= MAX_DIFF_LINES {
            break;
        }
        hunks.push(DiffLine {
            kind: "del".into(),
            text: line.to_string(),
        });
    }
    for line in new.lines() {
        if hunks.len() >= MAX_DIFF_LINES {
            break;
        }
        hunks.push(DiffLine {
            kind: "add".into(),
            text: line.to_string(),
        });
    }
    (added, removed, hunks)
}

fn compute_file_diff(old: &str, new: &str) -> (usize, usize, Vec<DiffLine>) {
    if old.is_empty() {
        let added = new.lines().count();
        let hunks: Vec<DiffLine> = new
            .lines()
            .take(MAX_DIFF_LINES)
            .map(|line| DiffLine {
                kind: "add".into(),
                text: line.to_string(),
            })
            .collect();
        return (added, 0, hunks);
    }

    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            if old_lines[i - 1] == new_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            ops.push((' ', old_lines[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(('+', new_lines[j - 1]));
            j -= 1;
        } else {
            ops.push(('-', old_lines[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();

    let mut added = 0usize;
    let mut removed = 0usize;
    let mut hunks = Vec::new();
    for (kind, text) in ops {
        if hunks.len() >= MAX_DIFF_LINES {
            break;
        }
        match kind {
            '+' => {
                added += 1;
                hunks.push(DiffLine {
                    kind: "add".into(),
                    text: text.to_string(),
                });
            }
            '-' => {
                removed += 1;
                hunks.push(DiffLine {
                    kind: "del".into(),
                    text: text.to_string(),
                });
            }
            _ => {
                hunks.push(DiffLine {
                    kind: "ctx".into(),
                    text: text.to_string(),
                });
            }
        }
    }
    (added, removed, hunks)
}

fn cursor_file_edit(v: &serde_json::Value, call_id: &str) -> Option<ActivityEvent> {
    let tool_call = v.get("tool_call")?;
    let obj = tool_call.as_object()?;
    for (key, val) in obj {
        match key.as_str() {
            "writeToolCall" => {
                let path = val.pointer("/args/path")?.as_str()?.to_string();
                let new = val
                    .pointer("/args/fileText")
                    .or_else(|| val.pointer("/args/content"))
                    .or_else(|| val.pointer("/args/streamContent"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let (lines_added, lines_removed, hunks) = compute_file_diff("", new);
                return Some(ActivityEvent::FileEdit {
                    id: call_id.to_string(),
                    path: file_basename(&path),
                    lines_added,
                    lines_removed,
                    hunks,
                });
            }
            "searchReplaceToolCall" | "editToolCall" | "editFileToolCall" => {
                let path = val.pointer("/args/path")?.as_str()?.to_string();
                let old = val
                    .pointer("/args/oldString")
                    .or_else(|| val.pointer("/args/old_string"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let new = val
                    .pointer("/args/newString")
                    .or_else(|| val.pointer("/args/new_string"))
                    .or_else(|| val.pointer("/args/streamContent"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let (lines_added, lines_removed, hunks) = compute_replace_diff(old, new);
                return Some(ActivityEvent::FileEdit {
                    id: call_id.to_string(),
                    path: file_basename(&path),
                    lines_added,
                    lines_removed,
                    hunks,
                });
            }
            _ => {}
        }
    }
    None
}

#[async_trait]
impl Provider for CliProvider {
    async fn chat(
        &self,
        req: &ChatRequest,
        sink: Option<&EventSink>,
    ) -> Result<ChatResponse, String> {
        if let Some(remote) = self.remote.clone() {
            return self.chat_remote(req, sink, remote).await;
        }
        let resumable = matches!(self.kind.as_str(), "antigravity_cli" | "claude_code");
        let existing_agy_conv = if resumable && !req.session_id.is_empty() {
            get_cli_conversation(&req.session_id)
        } else {
            None
        };
        // A resumed CLI already holds the thread, so replaying it would send the whole
        // history a second time — send only what is new.
        let is_resumed_agy = existing_agy_conv.is_some();
        let prompt = Self::build_prompt(req, is_resumed_agy);
        let prompt_tokens_est = crate::ai::text::count_tokens(&prompt) as u32;
        let stream_json = (self.kind == "cursor" && req.xconsole.is_some())
            || matches!(self.kind.as_str(), "antigravity_cli" | "claude_code");

        let workspace = if let Some(xc) = &req.xconsole {
            let label = match self.kind.as_str() {
                "antigravity_cli" => {
                    "Starting Antigravity CLI with xConsole MCP (SSH to your VPS)…"
                }
                "cursor" => "Starting Cursor with xConsole MCP (SSH to your VPS)…",
                "claude_code" => "Starting Claude Code with xConsole MCP (SSH to your VPS)…",
                _ => "Starting agent with xConsole MCP (SSH to your VPS)…",
            };
            emit(sink, StreamEvent::Status(label.into()));
            Some(
                prepare_agent_workspace(
                    &xc.data_dir,
                    &xc.session_id,
                    &xc.targets,
                    &xc.safety,
                    &xc.workspace_id,
                )
                .map_err(|e| format!("failed to prepare MCP workspace: {e}"))?,
            )
        } else {
            None
        };

        let flags = self.run_flags(
            req.xconsole.as_ref(),
            workspace.as_deref(),
            existing_agy_conv.as_deref(),
            &req.reasoning,
        );
        let key = self.api_key.as_deref();
        let bin = resolve_models_bin(&self.kind, &self.bin);

        let child = spawn_with_stdin(
            &self.kind,
            &bin,
            &flags,
            &prompt,
            key,
            workspace.as_deref(),
        )
        .await?;

        let started = std::time::Instant::now();
        let resp = read_child_output(
            child,
            &self.bin,
            &self.kind,
            &req.session_id,
            sink,
            stream_json,
            req.cancel.clone(),
        )
        .await?;

        let completion_tokens = resp.completion_tokens.unwrap_or_else(|| {
            crate::ai::text::count_tokens(&resp.content) as u32
        });
        let prompt_tokens = resp.prompt_tokens.unwrap_or(prompt_tokens_est);
        let cached_tokens = resp.cached_tokens;
        let ms = started.elapsed().as_millis() as u64;
        let secs = (ms as f64 / 1000.0).max(0.05);
        emit(
            sink,
            StreamEvent::Stats(crate::ai::provider::StreamStats {
                completion_tokens,
                prompt_tokens: Some(prompt_tokens),
                cached_tokens,
                cache_creation_tokens: None,
                duration_ms: ms,
                tokens_per_sec: (completion_tokens as f64 / secs) as f32,
            }),
        );
        Ok(resp)
    }

    fn is_autonomous_cli(&self) -> bool {
        true
    }
}

fn login_args(kind: &str) -> Vec<String> {
    match kind {
        "opencode_cli" => vec!["auth".into(), "login".into()],
        // `claude login` is terminal-only and unavailable under -p. `setup-token` is the
        // documented way to authorise a non-interactive caller from a subscription.
        "claude_code" => vec!["setup-token".into()],
        // `grok login` alone opens a browser, which a spawned child with no terminal
        // cannot drive. `--device-auth` is documented as the headless/remote path: it
        // prints a code for the user to enter elsewhere, and that code can be streamed
        // back to them. (Verified against `grok login --help`, v1.0.13.)
        "grok_cli" => vec!["login".into(), "--device-auth".into()],
        _ => vec!["login".into()],
    }
}

/// The bare command name an agent CLI installs as, with no filesystem probing.
///
/// This is what a remote run starts from: nothing about this machine's disk says
/// anything about the server's.
pub fn remote_default_bin(kind: &str) -> &'static str {
    match kind {
        "opencode_cli" => "opencode",
        "antigravity_cli" => "agy",
        "cursor" => "agent",
        "claude_code" => "claude",
        "grok_cli" => "grok",
        _ => "codex",
    }
}

/// Pure half of [`CliProvider::remote_bin`], with this machine's home passed in so it
/// can be tested without touching the environment.
pub fn remote_bin_for(kind: &str, bin: &str, local_home: Option<&str>) -> String {
    let bin = bin.trim();
    if bin.is_empty() {
        return remote_default_bin(kind).to_string();
    }
    let looks_local = local_home
        .map(|h| !h.is_empty() && bin.starts_with(h))
        .unwrap_or(false);
    if looks_local {
        return remote_default_bin(kind).to_string();
    }
    bin.to_string()
}

pub fn is_cli_kind(kind: &str) -> bool {
    matches!(
        kind,
        "codex_cli" | "opencode_cli" | "cursor" | "antigravity_cli" | "claude_code" | "grok_cli"
    )
}

/// Run `opencode models` / `agy models` and return available model IDs.
pub async fn list_models(kind: &str, bin: &str) -> Result<Vec<String>, String> {
    let args: Vec<String> = match kind {
        "opencode_cli" | "antigravity_cli" => vec!["models".into()],
        _ => return Ok(Vec::new()),
    };
    let bin = resolve_models_bin(kind, bin);

    let mut cmd = spawn_cli_program(&bin)?;
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    crate::proc::hide_console(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch '{bin}': {e}"))?;

    let timeout = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        child.wait_with_output(),
    )
    .await;

    let output = match timeout {
        Ok(Ok(out)) => out,
        _ => {
            if kind == "antigravity_cli" {
                return Ok(vec![
                    "gemini-3.7-flash-high".into(),
                    "gemini-3.7-flash-medium".into(),
                    "gemini-3.7-flash-low".into(),
                    "gemini-3.6-flash-high".into(),
                    "gemini-3.6-flash-medium".into(),
                    "gemini-3.6-flash-low".into(),
                    "gemini-3.5-flash-high".into(),
                    "gemini-3.5-flash-medium".into(),
                    "gemini-3.5-flash-low".into(),
                    "gemini-3.1-pro-high".into(),
                    "gemini-3.1-pro-low".into(),
                    "claude-sonnet-4-6".into(),
                    "claude-opus-4-6-thinking".into(),
                    "gpt-oss-120b-medium".into(),
                ]);
            }
            return Ok(Vec::new());
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_cli_model_ids(&stdout);
    if parsed.is_empty() && kind == "antigravity_cli" {
        return Ok(vec![
            "gemini-3.7-flash-high".into(),
            "gemini-3.7-flash-medium".into(),
            "gemini-3.7-flash-low".into(),
            "gemini-3.6-flash-high".into(),
            "gemini-3.6-flash-medium".into(),
            "gemini-3.6-flash-low".into(),
            "gemini-3.5-flash-high".into(),
            "gemini-3.5-flash-medium".into(),
            "gemini-3.5-flash-low".into(),
            "gemini-3.1-pro-high".into(),
            "gemini-3.1-pro-low".into(),
            "claude-sonnet-4-6".into(),
            "claude-opus-4-6-thinking".into(),
            "gpt-oss-120b-medium".into(),
        ]);
    }
    Ok(parsed)
}

/// Saved providers may still point at the IDE (`antigravity-ide`). Model listing
/// and print-mode chat live on `agy`.
fn resolve_models_bin(kind: &str, bin: &str) -> String {
    if kind == "antigravity_cli" {
        let lower = bin.to_ascii_lowercase();
        if lower == "agy"
            || lower == "antigravity"
            || lower.contains("antigravity-ide")
            || bin.trim().is_empty()
            || !Path::new(bin).exists()
        {
            let def = CliProvider::default_bin(kind);
            if Path::new(&def).exists() {
                return def;
            }
        }
    }
    bin.to_string()
}

/// `agy models` prints `id<TAB>Display Name`. `opencode models` prints one id per line.
pub fn parse_cli_model_ids(stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("fetching")
            || lower.starts_with("usage")
            || lower.starts_with("available")
            || lower.starts_with("id")
            || line.starts_with('-')
        {
            continue;
        }
        let id = line.split('\t').next().unwrap_or(line).trim();
        if id.is_empty() || id.contains(' ') {
            continue;
        }
        if !out.iter().any(|e| e == id) {
            out.push(id.to_string());
        }
    }
    out
}

pub async fn login(kind: &str, bin: &str, sink: Option<&EventSink>) -> Result<String, String> {
    let args = login_args(kind);
    emit(
        sink,
        StreamEvent::Status(format!("Launching `{} {}`...", bin, args.join(" "))),
    );

    let mut child = if kind == "cursor" {
        let mut c = cursor_base_command(bin);
        c.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        c.spawn().map_err(|e| {
            format!("failed to launch Cursor agent: {e}. Install/repair the CLI from https://cursor.com/docs/cli")
        })?
    } else {
        spawn_cli(bin, &args)?
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to launch '{bin}': {e}"))?
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let drain = |reader: Option<tokio::process::ChildStdout>| async move {
        let mut buf = String::new();
        if let Some(r) = reader {
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                buf.push_str(&line);
                buf.push('\n');
                emit(sink, StreamEvent::Text(format!("{line}\n")));
            }
        }
        buf
    };
    let drain_err = |reader: Option<tokio::process::ChildStderr>| async move {
        let mut buf = String::new();
        if let Some(r) = reader {
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                buf.push_str(&line);
                buf.push('\n');
                emit(sink, StreamEvent::Text(format!("{line}\n")));
            }
        }
        buf
    };
    // Drain both pipes concurrently to avoid a stderr-before-stdout-EOF deadlock.
    let (out_s, err_s) = tokio::join!(drain(stdout), drain_err(stderr));
    let combined = format!("{out_s}{err_s}");
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "login exited with {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agy_tab_separated_gemini_ids() {
        let out = "Fetching available models...\n\
gemini-3.7-flash-high\tGemini 3.7 Flash (High)\n\
gemini-3.1-pro-low\tGemini 3.1 Pro (Low)\n\
claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n";
        assert_eq!(
            parse_cli_model_ids(out),
            vec![
                "gemini-3.7-flash-high",
                "gemini-3.1-pro-low",
                "claude-sonnet-4-6",
            ]
        );
    }

    fn claude_code() -> CliProvider {
        CliProvider::new(
            "claude_code".into(),
            "claude".into(),
            Some("claude-opus-5".into()),
            None,
        )
    }

    #[test]
    fn claude_code_runs_headless_without_demanding_an_api_key() {
        let flags = claude_code().run_flags(None, None, None, "");
        let joined = flags.join(" ");
        assert!(joined.starts_with("-p "), "must be the print/headless entry point");
        assert!(joined.contains("--output-format stream-json"));
        // stream-json is rejected without it.
        assert!(flags.iter().any(|f| f == "--verbose"));
        assert!(joined.contains("--model claude-opus-5"));
        // `--bare` refuses to read the OAuth credentials and would force an API key on
        // a subscription user — the exact reason this provider exists.
        assert!(!flags.iter().any(|f| f == "--bare"));
        // Nobody is watching a permission prompt in a background turn.
        assert!(joined.contains("--permission-mode dontAsk"));
    }

    #[test]
    fn claude_code_resumes_a_known_thread_instead_of_starting_over() {
        let flags = claude_code().run_flags(None, None, Some("sess-123"), "");
        assert!(flags.join(" ").contains("--resume sess-123"));
    }

    #[test]
    fn claude_code_forwards_only_effort_levels_it_accepts() {
        assert!(claude_code().run_flags(None, None, None, "high").join(" ").contains("--effort high"));
        // xConsole has reasoning values Claude Code does not take; passing one through
        // would fail the whole run rather than just that setting.
        assert!(!claude_code().run_flags(None, None, None, "ludicrous").join(" ").contains("--effort"));
    }

    /// Lines captured from a real `claude -p --output-format stream-json --verbose` run.
    #[test]
    fn claude_code_stream_keeps_text_from_before_a_tool_call() {
        let mut out = ChatResponse::default();
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"abc-1","model":"claude-opus-5"}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"thinking","thinking":"hidden"},{"type":"text","text":"Checking. "}]}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"uptime"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"up 3 days"}]}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Up 3 days."}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Up 3 days.","session_id":"abc-1","usage":{"input_tokens":9,"output_tokens":39,"cache_read_input_tokens":13615}}"#,
        ];
        for line in lines {
            parse_claude_code_stream_line(line, "xc-session", &mut out, None);
        }

        // The Cursor parser replaces content per assistant message; doing that here would
        // lose "Checking. " the moment the tool call arrived.
        assert_eq!(out.content, "Checking. Up 3 days.");
        // Thinking blocks are not the answer and must not reach the transcript.
        assert!(!out.content.contains("hidden"));
        assert_eq!(out.prompt_tokens, Some(9));
        assert_eq!(out.completion_tokens, Some(39));
        assert_eq!(out.cached_tokens, Some(13615));
        // The id from `init` is what lets the next message resume this thread.
        assert_eq!(get_cli_conversation("xc-session").as_deref(), Some("abc-1"));
        remove_cli_conversation("xc-session");
    }

    #[test]
    fn claude_code_takes_the_result_line_when_nothing_streamed() {
        // A run that only produced a result (short answers sometimes arrive this way)
        // must not come back empty.
        let mut out = ChatResponse::default();
        parse_claude_code_stream_line(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"OK"}"#,
            "",
            &mut out,
            None,
        );
        assert_eq!(out.content, "OK");
    }

    #[test]
    fn claude_code_ignores_subagent_chatter() {
        // Subagent text carries the spawning call's id. Counting it as the answer would
        // splice a worker's monologue into what the user reads.
        let mut out = ChatResponse::default();
        parse_claude_code_stream_line(
            r#"{"type":"assistant","parent_tool_use_id":"t9","message":{"content":[{"type":"text","text":"inner"}]}}"#,
            "",
            &mut out,
            None,
        );
        assert!(out.content.is_empty());
    }

    #[test]
    fn parses_opencode_one_id_per_line() {
        assert_eq!(
            parse_cli_model_ids("opencode/big-pickle\nanthropic/claude-sonnet-4-5\n"),
            vec!["opencode/big-pickle", "anthropic/claude-sonnet-4-5"]
        );
    }
}
#[cfg(test)]
mod build_prompt_tests {
    use super::*;
    use crate::ai::context::RUNTIME_MARKER;
    use crate::ai::provider::ChatMessage;

    fn req(messages: Vec<ChatMessage>) -> ChatRequest {
        let mut r = ChatRequest::new("claude-opus-5");
        r.system = "You are the xConsole DevOps copilot.".into();
        r.messages = messages;
        r
    }

    fn runtime() -> ChatMessage {
        ChatMessage::user(format!(
            "{RUNTIME_MARKER}\nDate: Saturday\nDisk /: 600G / 698G — 87%"
        ))
    }

    #[test]
    fn a_vanished_conversation_is_told_apart_from_a_real_failure() {
        // Verified against the real CLI: resuming an id it no longer has exits 1 and
        // prints this on stderr, with no answer on stdout. The thread is gone either
        // way, so the only useful response is a fresh one — not an error handed to
        // somebody on a phone who cannot act on it.
        assert!(is_missing_conversation(
            "No conversation found with session ID: 00000000-1111-2222-3333-444444444444"
        ));
        assert!(is_missing_conversation("Session not found"));
        // Everything else is a real failure and must still be reported.
        assert!(!is_missing_conversation("Invalid API key"));
        assert!(!is_missing_conversation("bash: line 1: claude: command not found"));
        assert!(!is_missing_conversation(""));
    }

    #[test]
    fn a_resumed_turn_sends_what_the_user_typed() {
        // The bug: every request ends with a synthetic runtime block, so taking the
        // *last* user message sent the model context and none of the question. It
        // narrated the snapshot instead of answering, and on a turn with nothing worth
        // narrating it offered to help — which read as the agent ignoring the
        // conversation entirely.
        let p = CliProvider::build_prompt(
            &req(vec![ChatMessage::user("sterge fivem si txadmin"), runtime()]),
            true,
        );
        assert!(p.contains("sterge fivem si txadmin"), "the ask is missing: {p}");
        // Context still travels, but behind the ask, so the ask is what gets answered.
        assert!(p.contains("87%"), "runtime context should still be carried");
        assert!(
            p.trim_end().ends_with("sterge fivem si txadmin"),
            "the ask must come last: {p}"
        );
    }

    #[test]
    fn a_resumed_turn_with_no_context_sends_the_ask_alone() {
        // The CLI holds the thread on its side, so nothing else needs repeating.
        let p = CliProvider::build_prompt(
            &req(vec![ChatMessage::user("ce model esti?")]),
            true,
        );
        assert_eq!(p, "ce model esti?");
    }

    #[test]
    fn a_fresh_turn_does_not_label_the_runtime_block_as_the_user_speaking() {
        // "User: # Runtime context …" invites the model to answer the context.
        let p = CliProvider::build_prompt(
            &req(vec![ChatMessage::user("hi"), runtime()]),
            false,
        );
        assert!(p.contains("User: hi"));
        assert!(!p.contains(&format!("User: {RUNTIME_MARKER}")), "runtime is not an ask: {p}");
        assert!(p.contains(RUNTIME_MARKER), "runtime context should still be carried");
    }

    #[test]
    fn a_resumed_turn_with_only_context_has_nothing_to_ask() {
        // Nothing the user said: fall through rather than handing the model a block of
        // context and letting it invent a question to answer.
        let p = CliProvider::build_prompt(&req(vec![runtime()]), true);
        assert!(p.contains("You are the xConsole DevOps copilot."));
    }
}

