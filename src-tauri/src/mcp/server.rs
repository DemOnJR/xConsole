//! Minimal MCP stdio server (JSON-RPC, newline-delimited).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::ai::memory;
use crate::ai::safety;
use crate::ai::skills;
use crate::ai::workspace_context;
use crate::ai::AgentHome;
use crate::ssh::command::run_vps_command;
use crate::ssh::shell_quote;
use crate::storage::Db;

struct McpSession {
    db: Db,
    home: AgentHome,
    targets: Vec<String>,
    safety: String,
    /// Active workspace id (empty if none) — for the project brief / scoped memory.
    workspace_id: String,
    /// Shared dir the running app watches; canvas actions are dropped here as files
    /// (the MCP process can't emit Tauri events directly).
    queue_dir: PathBuf,
}

static CANVAS_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl McpSession {
    fn from_env() -> Result<Self, String> {
        let data_dir = std::env::var("XCONSOLE_DATA_DIR")
            .map_err(|_| "XCONSOLE_DATA_DIR not set".to_string())?;
        let agent_home = std::env::var("XCONSOLE_AGENT_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&data_dir).join("agent"));
        let targets = std::env::var("XCONSOLE_TARGETS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let safety = std::env::var("XCONSOLE_SAFETY").unwrap_or_else(|_| "approve".into());
        let workspace_id = std::env::var("XCONSOLE_WORKSPACE_ID").unwrap_or_default();

        let db_path = PathBuf::from(&data_dir).join("xconsole.db");
        let db = Db::open(&db_path).map_err(|e| format!("failed to open db: {e}"))?;

        Ok(Self {
            db,
            home: AgentHome::new(agent_home),
            targets,
            safety,
            workspace_id,
            queue_dir: PathBuf::from(&data_dir).join("canvas-queue"),
        })
    }

    /// Drop a canvas action file for the running app to pick up and forward.
    fn enqueue_canvas(&self, payload: Value) -> (String, bool) {
        if let Err(e) = std::fs::create_dir_all(&self.queue_dir) {
            return (format!("error: couldn't queue canvas action: {e}"), true);
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = CANVAS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = self.queue_dir.join(format!("{nanos}-{n}.json"));
        match std::fs::write(&path, serde_json::to_vec(&payload).unwrap_or_default()) {
            Ok(()) => ("done — updating the canvas now.".into(), false),
            Err(e) => (format!("error: couldn't queue canvas action: {e}"), true),
        }
    }

    fn tool_list(&self) -> Value {
        json!({
            "tools": [
                {
                    "name": "run_command",
                    "description": "Run a shell command on one of the user's VPS servers over SSH.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string" },
                            "vps_id": { "type": "string", "description": "Target VPS id; required when multiple targets are selected." }
                        },
                        "required": ["command"]
                    }
                },
                {
                    "name": "read_file",
                    "description": "Read a text file from a VPS.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "vps_id": { "type": "string" }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "write_file",
                    "description": "Write (overwrite) a text file on a VPS.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" },
                            "vps_id": { "type": "string" }
                        },
                        "required": ["path", "content"]
                    }
                },
                {
                    "name": "list_vps_targets",
                    "description": "List VPS targets available this session (id, name, host).",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "skills_list",
                    "description": "List available agent skills (playbooks).",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "skill_view",
                    "description": "Read a skill SKILL.md before using it.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "category": { "type": "string" },
                            "name": { "type": "string" }
                        },
                        "required": ["category", "name"]
                    }
                },
                {
                    "name": "skill_save",
                    "description": "Create or update a skill playbook.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "category": { "type": "string" },
                            "name": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["category", "name", "content"]
                    }
                },
                {
                    "name": "memory_save",
                    "description": "Save a durable fact to persistent agent memory.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "entry": { "type": "string" }
                        },
                        "required": ["entry"]
                    }
                },
                {
                    "name": "set_project_brief",
                    "description": "Write/replace the active workspace's project brief (CONTEXT.md) — \
                                    a concise overview of the project the agent keeps up to date. Use \
                                    this to initialize the brief on the first task in a workspace.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "content": { "type": "string" } },
                        "required": ["content"]
                    }
                },
                {
                    "name": "canvas_open_terminal",
                    "description": "Open a live terminal for a server on the xConsole canvas so the user can watch it.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "vps_id": { "type": "string" } },
                        "required": []
                    }
                },
                {
                    "name": "canvas_open_sftp",
                    "description": "Open an SFTP file-browser panel for a server on the xConsole canvas.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "vps_id": { "type": "string" } },
                        "required": []
                    }
                },
                {
                    "name": "canvas_tile",
                    "description": "Arrange the open canvas panels into a grid that fills the window.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "canvas_close",
                    "description": "Close a canvas panel. Pass node_id for one specific panel, or vps_id for all panels of a server.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "node_id": { "type": "string" }, "vps_id": { "type": "string" } },
                        "required": []
                    }
                },
                {
                    "name": "canvas_refresh",
                    "description": "Reconnect a terminal on the canvas (e.g. after the server rebooted). Pass node_id or vps_id.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "node_id": { "type": "string" }, "vps_id": { "type": "string" } },
                        "required": []
                    }
                }
            ]
        })
    }

    fn resolve_vps(&self, args: &Value) -> Result<String, String> {
        if self.targets.is_empty() {
            return Err("no VPS targets selected in xConsole agent panel".into());
        }
        if let Some(id) = args.get("vps_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            if !self.targets.iter().any(|t| t == id) {
                return Err(format!("vps_id '{id}' is not in the selected targets"));
            }
            return Ok(id.to_string());
        }
        match self.targets.len() {
            1 => Ok(self.targets[0].clone()),
            _ => Err(format!(
                "multiple VPS targets selected; pass vps_id (one of: {})",
                self.targets.join(", ")
            )),
        }
    }

    /// Effective safety mode for a target, honoring per-VPS overrides over the
    /// session default (so a server pinned to "approve" stays gated even when the
    /// global mode is "full"/"allowlist").
    fn effective_safety(&self, vps_id: Option<&str>) -> String {
        match vps_id {
            Some(id) => safety::effective_mode(&self.db, &self.safety, id),
            None => self.safety.clone(),
        }
    }

    fn allow_command(&self, command: &str, vps_id: Option<&str>) -> Result<(), String> {
        match self.effective_safety(vps_id).as_str() {
            "full" => Ok(()),
            "allowlist" if safety::is_allowlisted(command) => Ok(()),
            "allowlist" => {
                Err("command blocked by allowlist safety mode (use Full autonomy in xConsole)".into())
            }
            _ => Err(APPROVE_BLOCKED.into()),
        }
    }

    /// Authorize a read-only action (e.g. read_file). Gated on intent rather than
    /// re-parsing an assembled shell string, so paths containing shell
    /// metacharacters are read normally under allowlist mode.
    fn allow_read(&self, vps_id: Option<&str>) -> Result<(), String> {
        match self.effective_safety(vps_id).as_str() {
            "full" | "allowlist" => Ok(()),
            _ => Err(APPROVE_BLOCKED.into()),
        }
    }

    async fn tool_call(&self, name: &str, args: &Value) -> (String, bool) {
        match name {
            "list_vps_targets" => {
                let mut lines = Vec::new();
                for id in &self.targets {
                    if let Ok(Some(vps)) = self.db.get_vps(id) {
                        lines.push(format!("{} — {} ({}@{}:{})", id, vps.name, vps.username, vps.host, vps.port));
                    } else {
                        lines.push(format!("{id} — unknown"));
                    }
                }
                (lines.join("\n"), false)
            }
            "run_command" => {
                let command = match args.get("command").and_then(|v| v.as_str()) {
                    Some(c) if !c.is_empty() => c,
                    _ => return ("error: missing command".into(), true),
                };
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                if let Err(e) = self.allow_command(command, Some(&vps_id)) {
                    return (format!("error: {e}"), true);
                }
                match run_vps_command(&self.db, &vps_id, command).await {
                    Ok(out) => {
                        let mut s = format!("exit_code: {}\n", out.exit_code);
                        if !out.stdout.is_empty() {
                            s.push_str(&format!("stdout:\n{}\n", out.stdout.trim_end()));
                        }
                        if !out.stderr.is_empty() {
                            s.push_str(&format!("stderr:\n{}\n", out.stderr.trim_end()));
                        }
                        (s, out.exit_code != 0)
                    }
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "read_file" => {
                let path = match args.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p,
                    _ => return ("error: missing path".into(), true),
                };
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                let cmd = format!("cat -- {}", shell_quote(path));
                if let Err(e) = self.allow_read(Some(&vps_id)) {
                    return (format!("error: {e}"), true);
                }
                match run_vps_command(&self.db, &vps_id, &cmd).await {
                    Ok(out) => {
                        let text = if out.stdout.is_empty() {
                            format!("exit_code: {}\nstderr:\n{}", out.exit_code, out.stderr)
                        } else {
                            out.stdout
                        };
                        (text, out.exit_code != 0)
                    }
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "write_file" => {
                let path = match args.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p,
                    _ => return ("error: missing path".into(), true),
                };
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                let b64 = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    content.as_bytes(),
                );
                let cmd = format!(
                    "printf %s {} | base64 -d > {}",
                    shell_quote(&b64),
                    shell_quote(path)
                );
                if let Err(e) = self.allow_command(&cmd, Some(&vps_id)) {
                    return (format!("error: {e}"), true);
                }
                match run_vps_command(&self.db, &vps_id, &cmd).await {
                    Ok(out) => (
                        format!("exit_code: {}\n{}", out.exit_code, out.stderr.trim()),
                        out.exit_code != 0,
                    ),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "skills_list" => {
                let skills = skills::discover(&self.home);
                if skills.is_empty() {
                    return ("no skills installed".into(), false);
                }
                let text = skills
                    .iter()
                    .map(|s| format!("{}/{} — {}", s.category, s.name, s.description))
                    .collect::<Vec<_>>()
                    .join("\n");
                (text, false)
            }
            "skill_view" => {
                let cat = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                match skills::read_skill(&self.home, cat, name) {
                    Some(body) => (body, false),
                    None => (format!("error: skill '{cat}/{name}' not found"), true),
                }
            }
            "skill_save" => {
                let cat = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match skills::save_skill(&self.home, cat, name, content) {
                    Ok(()) => (format!("saved skill {cat}/{name}"), false),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "memory_save" => {
                let entry = args.get("entry").and_then(|v| v.as_str()).unwrap_or("");
                if entry.trim().is_empty() {
                    return ("error: missing entry".into(), true);
                }
                // Scope to the active workspace when there is one (else global memory).
                let result = if self.workspace_id.is_empty() {
                    memory::append_memory(&self.home, entry).map(|_| ())
                } else {
                    workspace_context::append_memory(&self.home, &self.workspace_id, entry)
                };
                match result {
                    Ok(()) => ("saved to memory".into(), false),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "set_project_brief" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if self.workspace_id.is_empty() {
                    return (
                        "error: no active workspace — the project brief is per-workspace. Ask the \
                         user to select a workspace first."
                            .into(),
                        true,
                    );
                }
                match workspace_context::save_brief(&self.home, &self.workspace_id, content) {
                    Ok(()) => ("saved the project brief for this workspace".into(), false),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "canvas_open_terminal" | "canvas_open_sftp" => {
                let action = if name == "canvas_open_terminal" {
                    "open_terminal"
                } else {
                    "open_sftp"
                };
                match self.resolve_vps(args) {
                    Ok(vps_id) => self.enqueue_canvas(json!({ "action": action, "vps_id": vps_id })),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "canvas_tile" => self.enqueue_canvas(json!({ "action": "tile" })),
            "canvas_close" | "canvas_refresh" => {
                let action = if name == "canvas_close" { "close" } else { "reconnect" };
                if let Some(node_id) =
                    args.get("node_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
                {
                    self.enqueue_canvas(json!({ "action": action, "node_id": node_id }))
                } else {
                    match self.resolve_vps(args) {
                        Ok(vps_id) => {
                            self.enqueue_canvas(json!({ "action": action, "vps_id": vps_id }))
                        }
                        Err(e) => (format!("error: {e}"), true),
                    }
                }
            }
            other => (format!("error: unknown tool '{other}'"), true),
        }
    }
}

const APPROVE_BLOCKED: &str =
    "command blocked: xConsole safety is Approve mode; switch to Full or Allowlist for Cursor MCP";

pub fn run_stdio_server() -> Result<(), String> {
    let session = Arc::new(McpSession::from_env()?);
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // A malformed line must not kill the server: reply with a JSON-RPC
        // parse error and keep serving (a fatal return here exits the process).
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let resp = json_response(
                    Value::Null,
                    None,
                    Some(json!({ "code": -32700, "message": format!("parse error: {e}") })),
                );
                writeln!(stdout, "{}", resp).map_err(|e| e.to_string())?;
                stdout.flush().map_err(|e| e.to_string())?;
                continue;
            }
        };
        if let Some(resp) = handle_message(&session, &msg, &rt) {
            writeln!(stdout, "{}", resp).map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn handle_message(session: &Arc<McpSession>, msg: &Value, rt: &tokio::runtime::Runtime) -> Option<String> {
    let method = msg.get("method")?.as_str()?;
    if method.starts_with("notifications/") {
        return None;
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "xconsole", "version": env!("CARGO_PKG_VERSION") }
        }),
        "tools/list" => session.tool_list(),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, is_error) = rt.block_on(session.tool_call(name, &args));
            json!({
                "content": [{ "type": "text", "text": text }],
                "isError": is_error
            })
        }
        "ping" => json!({}),
        _ => {
            return Some(json_response(
                id,
                None,
                Some(json!({ "code": -32601, "message": format!("method not found: {method}") })),
            ));
        }
    };

    Some(json_response(id, Some(result), None))
}

fn json_response(id: Value, result: Option<Value>, error: Option<Value>) -> String {
    let mut obj = json!({ "jsonrpc": "2.0", "id": id });
    if let Some(r) = result {
        obj["result"] = r;
    }
    if let Some(e) = error {
        obj["error"] = e;
    }
    obj.to_string()
}
