//! Local CLI agent providers (Codex / OpenCode / Cursor Agent).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::ai::provider::{
    emit, ActivityEvent, ChatRequest, ChatResponse, DiffLine, EventSink, Provider, StreamEvent,
    XConsoleExec,
};
use crate::mcp::prepare_cursor_workspace;

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
    fn run_flags(&self, xconsole: Option<&XConsoleExec>, workspace: Option<&Path>) -> Vec<String> {
        match self.kind.as_str() {
            "opencode_cli" => {
                let mut a = vec!["run".to_string()];
                if let Some(m) = &self.model {
                    a.push("--model".into());
                    a.push(m.clone());
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

    fn build_prompt(req: &ChatRequest) -> String {
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
        let mut c = Command::new(node);
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
                let mut c = Command::new("cmd");
                c.arg("/C").arg(bin);
                return c;
            }
            if ext == "ps1" {
                let mut c = Command::new("powershell");
                c.arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass").arg("-File").arg(bin);
                return c;
            }
        }
        // Bare name like "agent": resolve the known install location's .cmd.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let cmd_path = format!(r"{local}\cursor-agent\agent.cmd");
            if std::path::Path::new(&cmd_path).exists() {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(cmd_path);
                return c;
            }
        }
    }
    Command::new(bin)
}

/// Spawn a CLI process. Prompt is written to stdin when `prompt` is Some.
async fn spawn_with_stdin(
    kind: &str,
    bin: &str,
    flags: &[String],
    prompt: &str,
    api_key: Option<&str>,
) -> Result<tokio::process::Child, String> {
    let mut cmd = if kind == "cursor" {
        cursor_base_command(bin)
    } else {
        spawn_cli_program(bin)?
    };

    cmd.args(flags)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if kind == "cursor" {
        if let Some(key) = api_key {
            cmd.env("CURSOR_API_KEY", key);
        }
    }

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
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(bin);
            return Ok(cmd);
        }
    }
    Ok(Command::new(bin))
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
    let stdout_fut = async move {
        let mut out = ChatResponse::default();
        if let Some(stdout) = stdout {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                tokio::select! {
                    next = lines.next_line() => match next {
                        Ok(Some(line)) => {
                            if stream_json {
                                parse_cursor_stream_line(&line, &mut out, sink);
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
        let hint = if kind == "cursor"
            && (err.contains("invalid") || err.contains("Not logged in") || err.contains("API key"))
        {
            " Check your API key or use Login in Settings → Providers."
        } else {
            ""
        };
        return Err(format!(
            "{} exited with {}: {}{}",
            bin,
            status.code().unwrap_or(-1),
            err.trim(),
            hint
        ));
    }

    out.stop_reason = "stop".into();
    Ok(out)
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
        let prompt = Self::build_prompt(req);
        // Count prompt tokens now (the prompt is moved into the child args below).
        let prompt_tokens = crate::ai::text::count_tokens(&prompt) as u32;
        let stream_json = self.kind == "cursor" && req.xconsole.is_some();

        let workspace = if let Some(xc) = &req.xconsole {
            emit(
                sink,
                StreamEvent::Status(
                    "Starting Cursor with xConsole MCP (SSH to your VPS)…".into(),
                ),
            );
            Some(
                prepare_cursor_workspace(
                    &xc.data_dir,
                    &xc.session_id,
                    &xc.targets,
                    &xc.safety,
                    &xc.workspace_id,
                )
                .map_err(|e| format!("failed to prepare Cursor MCP workspace: {e}"))?,
            )
        } else {
            None
        };

        let flags = self.run_flags(req.xconsole.as_ref(), workspace.as_deref());
        let key = self.api_key.as_deref();

        let child = if self.kind == "cursor" {
            spawn_with_stdin(&self.kind, &self.bin, &flags, &prompt, key).await?
        } else {
            let mut args = flags;
            args.push(prompt);
            spawn_cli(&self.bin, &args)?
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("failed to launch '{}': {e}", self.bin))?
        };

        let started = std::time::Instant::now();
        let resp =
            read_child_output(child, &self.bin, &self.kind, sink, stream_json, req.cancel.clone())
                .await?;

        // CLI tools don't report token usage — tokenize locally so the calculator
        // still works (labeled as an estimate in the UI).
        let completion_tokens = crate::ai::text::count_tokens(&resp.content) as u32;
        let ms = started.elapsed().as_millis() as u64;
        let secs = (ms as f64 / 1000.0).max(0.05);
        emit(
            sink,
            StreamEvent::Stats(crate::ai::provider::StreamStats {
                completion_tokens,
                prompt_tokens: Some(prompt_tokens),
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
    matches!(kind, "codex_cli" | "opencode_cli" | "cursor")
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
