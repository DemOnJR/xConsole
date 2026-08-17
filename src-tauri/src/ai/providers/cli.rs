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

static AGY_SESSIONS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_agy_conversation(session_id: &str) -> Option<String> {
    if session_id.is_empty() {
        return None;
    }
    AGY_SESSIONS.lock().ok()?.get(session_id).cloned()
}

fn store_agy_conversation(session_id: &str, conversation_id: &str) {
    if session_id.is_empty() || conversation_id.is_empty() {
        return;
    }
    if let Ok(mut map) = AGY_SESSIONS.lock() {
        map.insert(session_id.to_string(), conversation_id.to_string());
    }
}

fn remove_agy_conversation(session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    if let Ok(mut map) = AGY_SESSIONS.lock() {
        map.remove(session_id);
    }
}

pub struct CliProvider {
    kind: String,
    bin: String,
    model: Option<String>,
    api_key: Option<String>,
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
        }
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

    fn build_prompt(req: &ChatRequest, is_resumed_agy: bool) -> String {
        if is_resumed_agy {
            if let Some(last_user) = req.messages.iter().rev().find(|m| m.role == "user") {
                return last_user.content.clone();
            }
        }
        let mut s = String::new();
        if !req.system.is_empty() {
            s.push_str(&req.system);
            s.push_str("\n\n");
        }
        for m in &req.messages {
            match m.role.as_str() {
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

    cmd.args(flags)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if kind == "cursor" {
        if let Some(key) = api_key {
            cmd.env("CURSOR_API_KEY", key);
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
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|e| format!("failed to write prompt to CLI stdin: {e}"))?;
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
                                if kind_owned == "antigravity_cli" {
                                    parse_antigravity_stream_line(&line, &session_id_owned, &mut out, sink);
                                } else {
                                    parse_cursor_stream_line(&line, &mut out, sink);
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
        if kind == "antigravity_cli" {
            remove_agy_conversation(session_id);
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
                    store_agy_conversation(session_id, conv_id);
                }
            }
        }
        "step_update" => {
            let Some(su) = v.get("step_update") else {
                return;
            };
            if let Some(conv_id) = su.get("conversation_id").and_then(|c| c.as_str()) {
                if !conv_id.is_empty() && !session_id.is_empty() {
                    store_agy_conversation(session_id, conv_id);
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
                    store_agy_conversation(session_id, conv_id);
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
        let existing_agy_conv = if self.kind == "antigravity_cli" && !req.session_id.is_empty() {
            get_agy_conversation(&req.session_id)
        } else {
            None
        };
        let is_resumed_agy = existing_agy_conv.is_some();
        let prompt = Self::build_prompt(req, is_resumed_agy);
        let prompt_tokens_est = crate::ai::text::count_tokens(&prompt) as u32;
        let stream_json = (self.kind == "cursor" && req.xconsole.is_some())
            || self.kind == "antigravity_cli";

        let workspace = if let Some(xc) = &req.xconsole {
            let label = match self.kind.as_str() {
                "antigravity_cli" => {
                    "Starting Antigravity CLI with xConsole MCP (SSH to your VPS)…"
                }
                "cursor" => "Starting Cursor with xConsole MCP (SSH to your VPS)…",
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
        _ => vec!["login".into()],
    }
}

pub fn is_cli_kind(kind: &str) -> bool {
    matches!(kind, "codex_cli" | "opencode_cli" | "cursor" | "antigravity_cli")
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
    use super::parse_cli_model_ids;

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

    #[test]
    fn parses_opencode_one_id_per_line() {
        assert_eq!(
            parse_cli_model_ids("opencode/big-pickle\nanthropic/claude-sonnet-4-5\n"),
            vec!["opencode/big-pickle", "anthropic/claude-sonnet-4-5"]
        );
    }
}
