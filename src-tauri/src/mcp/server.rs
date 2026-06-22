//! Minimal MCP stdio server (JSON-RPC, newline-delimited).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::ai::memory;
use crate::ai::safety;
use crate::ai::skills;
use crate::ai::AgentHome;
use crate::ssh::command::run_vps_command;
use crate::storage::Db;

struct McpSession {
    db: Db,
    home: AgentHome,
    targets: Vec<String>,
    safety: String,
}

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

        let db_path = PathBuf::from(&data_dir).join("xconsole.db");
        let db = Db::open(&db_path).map_err(|e| format!("failed to open db: {e}"))?;

        Ok(Self {
            db,
            home: AgentHome::new(agent_home),
            targets,
            safety,
        })
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

    fn allow_command(&self, command: &str, _vps_id: Option<&str>) -> Result<(), String> {
        let mode = self.safety.as_str();
        match mode {
            "full" => Ok(()),
            "allowlist" => {
                if safety::is_read_only(command) || safety::is_terraform_readonly(command) {
                    Ok(())
                } else {
                    Err("command blocked by allowlist safety mode (use Full autonomy in xConsole)".into())
                }
            }
            _ => Err("command blocked: xConsole safety is Approve mode; switch to Full or Allowlist for Cursor MCP".into()),
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
                if let Err(e) = self.allow_command(&cmd, Some(&vps_id)) {
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
                match memory::append_memory(&self.home, entry) {
                    Ok(_) => ("saved to memory".into(), false),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            other => (format!("error: unknown tool '{other}'"), true),
        }
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

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
        let msg: Value = serde_json::from_str(line).map_err(|e| format!("invalid json: {e}"))?;
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
