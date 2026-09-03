//! High-Performance Async Model Context Protocol (MCP) Server.
//!
//! Provides JSON-RPC 2.0 over stdio and reverse-tunnel streams for external AI
//! agents (Claude Code, Cursor Agent CLI, Antigravity, Aider, Grok).

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::ai::memory;
use crate::ai::safety;
use crate::ai::skills;
use crate::ai::workspace_context;
use crate::ai::AgentHome;
use crate::ssh::command::run_vps_command;
use crate::ssh::shell_quote;
use crate::storage::Db;

const MAX_OUTPUT_BYTES: usize = 64 * 1024; // 64 KB safety cap per return payload

fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s.to_string();
    }
    let boundary = match s
        .char_indices()
        .take_while(|(idx, _)| *idx < MAX_OUTPUT_BYTES)
        .last()
    {
        Some((idx, _)) => idx,
        None => MAX_OUTPUT_BYTES,
    };
    let slice = &s[..boundary];
    let omitted_bytes = s.len() - boundary;
    let omitted_lines = s[boundary..].lines().count();
    format!(
        "{slice}\n\n[Output truncated: {omitted_lines} lines ({omitted_bytes} bytes) omitted. Narrow it: read_file with offset/limit for a window of a file, grep_search to find the line first]"
    )
}

/// One tool this bridge serves.
///
/// The schema and the prose come from [`crate::ai::tools::definitions`]; this only says
/// which tools exist here and how this transport differs from the in-app one.
struct BridgedTool {
    /// The name in `tools::definitions()`, and the name the handler below answers to.
    name: &'static str,
    /// Argument keys this transport's handler actually reads. Anything else in the
    /// shared schema is dropped, because advertising a parameter that is silently
    /// ignored is how a model concludes a tool is broken.
    props: &'static [&'static str],
    /// Overrides the shared `required` list when this transport needs a different one.
    /// Empty means "keep whatever the shared schema requires, minus dropped keys".
    required: &'static [&'static str],
    /// Appended to the shared description. Only ever a real behavioural difference —
    /// never a restatement of what the tool does.
    note: &'static str,
}

const fn tool(
    name: &'static str,
    props: &'static [&'static str],
    required: &'static [&'static str],
    note: &'static str,
) -> BridgedTool {
    BridgedTool {
        name,
        props,
        required,
        note,
    }
}

/// Every tool `tool_call` below implements, in the order a session tends to need them.
///
/// Shorter than the in-app set on purpose: this transport has no job runner, no
/// interactive prompt to ask the user a question through, and no plan gate. A tool that
/// cannot work here is left out rather than advertised and refused at call time.
const BRIDGED: &[BridgedTool] = &[
    tool(
        "run_command",
        &["command", "vps_id"],
        &["command"],
        "Over this bridge there is no background mode and no job tools: `background` and \
`timeout_secs` are not accepted, and a command is cut off after about two minutes. For \
anything slower, start it detached yourself (`nohup … >/tmp/x.log 2>&1 &` or \
`systemd-run --unit=…`) and read the log back on a later call.",
    ),
    tool(
        "read_file",
        &["path", "offset", "limit", "vps_id"],
        // The shared schema declares no `required` at all, which reads as "every
        // argument is optional" — and a read_file with no path is not a call, it is a
        // parse failure waiting to happen.
        &["path"],
        "One file per call over this bridge — `paths` (the batch form) is not accepted \
here.",
    ),
    tool(
        "write_file",
        &["path", "content", "vps_id"],
        &["path", "content"],
        "Always a full overwrite over this bridge: `mode: append` is not accepted, so a \
file too large for one call has to be assembled with edit_file or shell redirection.",
    ),
    tool(
        "edit_file",
        &["path", "old_string", "new_string", "replace_all", "vps_id"],
        &["path", "old_string", "new_string"],
        "One replacement per call over this bridge — the `edits` batch form is not \
accepted here.",
    ),
    tool("list_dir", &["path", "all", "head_limit", "vps_id"], &[], ""),
    tool(
        "grep_search",
        &["pattern", "path", "case_insensitive", "head_limit", "vps_id"],
        &["pattern"],
        "This bridge runs plain `grep -rn`: `glob`, `context_lines`, `output_mode`, \
`multiline` and `offset` are not accepted. Narrow with `path` instead, then read_file \
with offset/limit around a hit.",
    ),
    tool(
        "find_files",
        &["pattern", "path", "type", "head_limit", "vps_id"],
        &["pattern"],
        "",
    ),
    tool("list_vps_targets", &[], &[], ""),
    tool("skills_list", &[], &[], ""),
    tool("skill_view", &["category", "name"], &["category", "name"], ""),
    tool(
        // No `category`: everything saved over this bridge lands in `unverified/`
        // regardless, so advertising a category the handler ignores would be a lie.
        "skill_save",
        &["name", "content"],
        &["name", "content"],
        "Saved unverified over this bridge, and only when xConsole's safety mode is Full: \
a skill written from a server is a standing instruction to every later agent, so it is \
quarantined until a person promotes it.",
    ),
    tool("memory_save", &["entry"], &["entry"], ""),
    tool("set_project_brief", &["content"], &["content"], ""),
    tool("host_memory_get", &["vps_id"], &["vps_id"], ""),
    tool(
        "host_memory_update",
        &["vps_id", "kind", "content"],
        &["vps_id", "kind", "content"],
        "",
    ),
    tool("canvas_open_terminal", &["vps_id"], &[], ""),
    tool("canvas_open_sftp", &["vps_id"], &[], ""),
    tool("canvas_tile", &[], &[], ""),
    tool("canvas_close", &["node_id", "vps_id"], &[], ""),
    tool("canvas_refresh", &["node_id", "vps_id"], &[], ""),
];

/// The shared description, plus this transport's difference where there is one.
fn bridged_description(shared: &str, note: &str) -> String {
    if note.is_empty() {
        shared.to_string()
    } else {
        format!("{shared}\n\nOn this connection: {note}")
    }
}

/// The shared schema, narrowed to the arguments this transport reads.
fn bridged_schema(shared: &Value, b: &BridgedTool) -> Value {
    let empty = serde_json::Map::new();
    let props = shared
        .get("properties")
        .and_then(|p| p.as_object())
        .unwrap_or(&empty);
    let mut kept = serde_json::Map::new();
    for key in b.props {
        if let Some(v) = props.get(*key) {
            kept.insert((*key).to_string(), v.clone());
        }
    }
    let required: Vec<Value> = if b.required.is_empty() {
        shared
            .get("required")
            .and_then(|r| r.as_array())
            .map(|r| {
                r.iter()
                    .filter(|v| v.as_str().map(|s| b.props.contains(&s)).unwrap_or(false))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    } else {
        b.required.iter().map(|s| json!(s)).collect()
    };
    json!({
        "type": "object",
        "properties": Value::Object(kept),
        "required": required,
    })
}

pub(crate) struct McpSession {
    db: Db,
    home: AgentHome,
    targets: Vec<String>,
    safety: String,
    /// Active workspace id (empty if none) — for the project brief / scoped memory.
    workspace_id: String,
    /// Shared dir the running app watches; canvas actions are dropped here as files.
    queue_dir: PathBuf,
    /// Ephemeral auth token when running over reverse tunnel
    token: Option<String>,
    /// Active task abort handles for client-driven request cancellation.
    abort_handles: Arc<DashMap<String, tokio::task::AbortHandle>>,
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
        let token = std::env::var("XCONSOLE_TOKEN").ok().filter(|s| !s.is_empty());

        let data_dir_path = PathBuf::from(&data_dir);
        let db_path = data_dir_path.join("xconsole.db");
        let db = if db_path.exists() {
            Db::open(&db_path).map_err(|e| format!("failed to open db: {e}"))?
        } else if crate::lock::is_lock_enabled(&data_dir_path) {
            match crate::secrets::get_data_key().ok().flatten() {
                Some(key) => Db::open_encrypted(
                    &data_dir_path.join("xconsole.db.enc"),
                    &db_path,
                    &data_dir_path,
                    &key,
                )
                .map_err(|e| format!("failed to open encrypted db: {e}"))?,
                None => {
                    return Err("xConsole is locked — open and unlock the app first, then retry.".into())
                }
            }
        } else {
            Db::open(&db_path).map_err(|e| format!("failed to open db: {e}"))?
        };

        Ok(Self {
            db,
            home: AgentHome::new(agent_home),
            targets,
            safety,
            workspace_id,
            queue_dir: PathBuf::from(&data_dir).join("canvas-queue"),
            token,
            abort_handles: Arc::new(DashMap::new()),
        })
    }

    /// Set the token this session will demand. Called by the transport that mints it.
    pub(crate) fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    /// The token every HTTP request must present. Empty when none is configured, which
    /// the HTTP transport treats as "refuse everything" rather than "allow everything".
    pub(crate) fn token_for_auth(&self) -> &str {
        self.token.as_deref().unwrap_or("")
    }

    /// Build a session inside the running app, rather than from a subprocess's
    /// environment.
    ///
    /// `from_env` exists because the stdio transport *is* a separate process. The HTTP
    /// transport runs in the app itself and already holds the db and agent home, so
    /// re-deriving them from env vars would mean re-opening (and possibly failing to
    /// unlock) a database this process has open.
    pub(crate) fn in_process(
        db: Db,
        home: AgentHome,
        data_dir: &std::path::Path,
        targets: Vec<String>,
        safety: String,
        workspace_id: String,
        token: String,
    ) -> Self {
        Self {
            db,
            home,
            targets,
            safety,
            workspace_id,
            queue_dir: data_dir.join("canvas-queue"),
            token: Some(token),
            abort_handles: Arc::new(DashMap::new()),
        }
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

    /// The tools this bridge serves, described the way the in-app agent sees them.
    ///
    /// These used to be hand-declared here, a second and much worse copy of schemas that
    /// already exist in [`crate::ai::tools::definitions`]. That copy is the one a Claude
    /// Code agent on the VPS actually reads, so every sentence written for the in-app
    /// agent — when a tool is the wrong choice, what the alternative is, what a
    /// parameter is bounded to — stopped at the app's edge and the remote agent got
    /// "Run a shell command directly on the user's remote Linux VPS over SSH." Worse, the
    /// two drifted: this copy advertised a `read_file` with no bound at all next to a
    /// separate `read_file_range`, so the obvious call was the one that could return a
    /// 200 MB log.
    ///
    /// Now there is one source. [`BRIDGED`] says which tools this transport implements
    /// and which arguments its handlers really read; everything a model reads comes from
    /// the shared definition, plus a note wherever this transport genuinely behaves
    /// differently from the in-app one. A description that promises a parameter this
    /// bridge ignores is worse than a terse one.
    fn tool_list(&self) -> Value {
        let defs = crate::ai::tools::definitions(&self.home);
        let tools: Vec<Value> = BRIDGED
            .iter()
            .filter_map(|b| {
                let def = defs.iter().find(|d| d.name == b.name)?;
                Some(json!({
                    "name": b.name,
                    "description": bridged_description(&def.description, b.note),
                    "inputSchema": bridged_schema(&def.parameters, b),
                }))
            })
            .collect();
        json!({ "tools": tools })
    }

    fn resource_list(&self) -> Value {
        let mut resources = Vec::new();

        for id in &self.targets {
            if let Ok(Some(vps)) = self.db.get_vps(id) {
                resources.push(json!({
                    "uri": format!("vps://{id}/sysinfo"),
                    "name": format!("System Stats ({})", vps.name),
                    "description": format!("Real-time OS, CPU, RAM and disk usage for {}", vps.name),
                    "mimeType": "text/plain"
                }));
                resources.push(json!({
                    "uri": format!("vps://{id}/profile"),
                    "name": format!("Host Profile ({})", vps.name),
                    "description": format!("Dossier and architecture notes for {}", vps.name),
                    "mimeType": "text/markdown"
                }));
                resources.push(json!({
                    "uri": format!("vps://{id}/memory"),
                    "name": format!("Host Memory ({})", vps.name),
                    "description": format!("Institutional learned facts for {}", vps.name),
                    "mimeType": "text/markdown"
                }));
            }
        }

        if !self.workspace_id.is_empty() {
            resources.push(json!({
                "uri": "workspace://brief",
                "name": "Workspace Project Brief",
                "description": "Active workspace project overview and architecture brief (CONTEXT.md)",
                "mimeType": "text/markdown"
            }));
        }

        let skills = skills::discover(&self.home);
        for s in skills {
            resources.push(json!({
                "uri": format!("skills://{}/{}", s.category, s.name),
                "name": format!("Skill: {}/{}", s.category, s.name),
                "description": s.description,
                "mimeType": "text/markdown"
            }));
        }

        json!({ "resources": resources })
    }

    fn resource_templates(&self) -> Value {
        json!({
            "resourceTemplates": [
                {
                    "uriTemplate": "vps://{vps_id}/sysinfo",
                    "name": "VPS System Stats",
                    "mimeType": "text/plain"
                },
                {
                    "uriTemplate": "vps://{vps_id}/profile",
                    "name": "VPS Host Profile",
                    "mimeType": "text/markdown"
                },
                {
                    "uriTemplate": "vps://{vps_id}/memory",
                    "name": "VPS Host Memory",
                    "mimeType": "text/markdown"
                },
                {
                    "uriTemplate": "skills://{category}/{name}",
                    "name": "Skill Playbook",
                    "mimeType": "text/markdown"
                }
            ]
        })
    }

    async fn resource_read(&self, uri: &str) -> (Value, bool) {
        if uri == "workspace://brief" {
            if self.workspace_id.is_empty() {
                return (
                    json!({ "error": "no active workspace configured" }),
                    true,
                );
            }
            let brief = workspace_context::load_brief(&self.home, &self.workspace_id);
            return (
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "text/markdown",
                        "text": brief
                    }]
                }),
                false,
            );
        }

        if let Some(rest) = uri.strip_prefix("skills://") {
            let mut parts = rest.splitn(2, '/');
            let cat = parts.next().unwrap_or("");
            let name = parts.next().unwrap_or("");
            if let Some(body) = skills::read_skill(&self.home, cat, name) {
                return (
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "text/markdown",
                            "text": body
                        }]
                    }),
                    false,
                );
            } else {
                return (json!({ "error": format!("skill '{rest}' not found") }), true);
            }
        }

        if let Some(rest) = uri.strip_prefix("vps://") {
            let mut parts = rest.splitn(2, '/');
            let vps_id = parts.next().unwrap_or("");
            let kind = parts.next().unwrap_or("");

            if !self.targets.iter().any(|t| t == vps_id) {
                return (json!({ "error": format!("target '{vps_id}' not found") }), true);
            }

            match kind {
                "profile" => {
                    let profile = crate::ai::host_memory::load_profile(&self.home, vps_id);
                    return (
                        json!({
                            "contents": [{
                                "uri": uri,
                                "mimeType": "text/markdown",
                                "text": profile
                            }]
                        }),
                        false,
                    );
                }
                "memory" => {
                    let memory = crate::ai::host_memory::load_memory(&self.home, vps_id);
                    return (
                        json!({
                            "contents": [{
                                "uri": uri,
                                "mimeType": "text/markdown",
                                "text": memory
                            }]
                        }),
                        false,
                    );
                }
                "sysinfo" => {
                    let cmd = "uname -a; echo '--- UPTIME ---'; uptime; echo '--- MEMORY ---'; free -h; echo '--- DISK ---'; df -h /";
                    match run_vps_command(&self.db, vps_id, cmd).await {
                        Ok(out) => {
                            return (
                                json!({
                                    "contents": [{
                                        "uri": uri,
                                        "mimeType": "text/plain",
                                        "text": out.stdout
                                    }]
                                }),
                                false,
                            );
                        }
                        Err(e) => return (json!({ "error": format!("could not query sysinfo: {e}") }), true),
                    }
                }
                _ => return (json!({ "error": format!("unknown resource kind '{kind}'") }), true),
            }
        }

        (json!({ "error": format!("unrecognized URI '{uri}'") }), true)
    }

    fn prompt_list(&self) -> Value {
        json!({
            "prompts": [
                {
                    "name": "diagnose_server",
                    "description": "Comprehensive server health diagnosis: checks uptime, CPU load, memory pressure, disk space, failed systemd services, and dmesg kernel errors.",
                    "arguments": [
                        { "name": "vps_id", "description": "Target VPS id to diagnose", "required": false }
                    ]
                },
                {
                    "name": "audit_web_stack",
                    "description": "Audit web servers (Nginx/Apache/Caddy), active virtual hosts, listening ports, reverse proxies, and SSL certificates.",
                    "arguments": [
                        { "name": "vps_id", "description": "Target VPS id", "required": false }
                    ]
                },
                {
                    "name": "inspect_docker",
                    "description": "Inspect running Docker containers, health status, container resource usage, and recent container errors.",
                    "arguments": [
                        { "name": "vps_id", "description": "Target VPS id", "required": false }
                    ]
                }
            ]
        })
    }

    fn prompt_get(&self, name: &str, args: &Value) -> Result<Value, String> {
        let vps_id = self.resolve_vps(args).unwrap_or_else(|_| "selected target".into());
        match name {
            "diagnose_server" => Ok(json!({
                "description": "Server health diagnosis",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "Please perform a complete diagnostic health check on VPS target '{vps_id}':\n\
                                 1. Check system load and uptime: `uptime`\n\
                                 2. Check memory usage: `free -h` and identify top memory processes: `ps aux --sort=-%mem | head -n 10`\n\
                                 3. Check disk space and inodes: `df -h` and `df -i`\n\
                                 4. Check failed systemd units: `systemctl --failed`\n\
                                 5. Check recent kernel errors: `dmesg -T -l err,crit,alert,emerg | tail -n 25`\n\
                                 Summarize any issues found and recommend corrective actions."
                            )
                        }
                    }
                ]
            })),
            "audit_web_stack" => Ok(json!({
                "description": "Web server and virtual host audit",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "Please audit the web server and domain configuration on VPS target '{vps_id}':\n\
                                 1. Check listening HTTP/HTTPS ports: `ss -tulpn | grep -E ':(80|443|8080|3000)'`\n\
                                 2. Inspect web server status (Nginx/Apache/Caddy/Traefik): `systemctl status nginx apache2 caddy --no-pager 2>/dev/null`\n\
                                 3. List active sites and config files (e.g. `/etc/nginx/sites-enabled/`, `/etc/nginx/conf.d/`, `/var/www/`)\n\
                                 4. Check SSL certificate paths and expiration dates.\n\
                                 Provide an overview of active domains, reverse proxy routes, and root folders."
                            )
                        }
                    }
                ]
            })),
            "inspect_docker" => Ok(json!({
                "description": "Docker container inspection",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "Please inspect Docker containers on VPS target '{vps_id}':\n\
                                 1. List running and stopped containers: `docker ps -a`\n\
                                 2. Check container CPU/RAM stats: `docker stats --no-stream`\n\
                                 3. Check docker compose files in `/root` or `/var/www`.\n\
                                 4. Inspect logs of any failing or restarting containers: `docker logs --tail 50 <container_id>`\n\
                                 Summarize the health of all containerized services."
                            )
                        }
                    }
                ]
            })),
            other => Err(format!("unknown prompt '{other}'")),
        }
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

    fn allow_read(&self, vps_id: Option<&str>, path: &str) -> Result<(), String> {
        if safety::touches_sensitive_path(path) {
            return Err(
                "that path looks like a credential store, so it needs explicit approval — \
                 read it from the xConsole app instead"
                    .into(),
            );
        }
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
                        (truncate_output(&s), out.exit_code != 0)
                    }
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            // `read_file_range` was a second tool that did what `read_file` should always
            // have done. Two entries meant the unbounded one was the obvious call and the
            // bounded one was the exception, so a 200 MB log was one plausible tool call
            // away. Now there is one tool with an optional window, and the old name is
            // still answered for any agent mid-session that learned it.
            "read_file" | "read_file_range" => {
                let path = match args.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p,
                    _ => return ("error: missing path".into(), true),
                };
                let offset = args.get("offset").and_then(|v| v.as_i64());
                let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32);
                let windowed = offset.is_some() || limit.is_some();
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                if let Err(e) = self.allow_read(Some(&vps_id), path) {
                    return (format!("error: {e}"), true);
                }
                let cmd = format!("cat -- {}", shell_quote(path));
                match run_vps_command(&self.db, &vps_id, &cmd).await {
                    Ok(out) if out.exit_code == 0 => {
                        // Line numbers whenever a window was asked for, because the next
                        // call is usually "now read around line N" and an unnumbered
                        // window cannot answer it.
                        let text = if windowed {
                            crate::ai::file_ops::format_read(&out.stdout, offset, limit)
                        } else {
                            out.stdout
                        };
                        (truncate_output(&text), false)
                    }
                    Ok(out) => (
                        format!("exit_code: {}\nstderr:\n{}", out.exit_code, out.stderr),
                        true,
                    ),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "edit_file" => {
                let path = match args.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p,
                    _ => return ("error: missing path".into(), true),
                };
                let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
                    Some(s) if !s.is_empty() => s,
                    _ => return ("error: missing old_string".into(), true),
                };
                let new_string = args.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                let replace_all = args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                if let Err(e) = self.allow_read(Some(&vps_id), path) {
                    return (format!("error: {e}"), true);
                }
                let read_cmd = format!("cat -- {}", shell_quote(path));
                let before_out = match run_vps_command(&self.db, &vps_id, &read_cmd).await {
                    Ok(o) => o,
                    Err(e) => return (format!("error reading file: {e}"), true),
                };
                if before_out.exit_code != 0 && before_out.stdout.is_empty() {
                    return (
                        format!("error: could not read {path} (exit code {}). Use write_file to create it.", before_out.exit_code),
                        true,
                    );
                }
                let (next_content, count) = match crate::ai::file_ops::apply_edit(&before_out.stdout, old_string, new_string, replace_all) {
                    Ok(res) => res,
                    Err(e) => return (e, true),
                };
                let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, next_content.as_bytes());
                let write_cmd = format!("printf %s {} | base64 -d > {}", shell_quote(&b64), shell_quote(path));
                if let Err(e) = self.allow_command(&write_cmd, Some(&vps_id)) {
                    return (format!("error: {e}"), true);
                }
                match run_vps_command(&self.db, &vps_id, &write_cmd).await {
                    Ok(out) if out.exit_code == 0 => (
                        format!("Successfully edited {path} ({count} occurrence(s) replaced)"),
                        false,
                    ),
                    Ok(out) => (format!("error writing {path}: exit code {}\n{}", out.exit_code, out.stderr), true),
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
            // `list_directory` was this transport's own name for the tool the rest of
            // the app calls `list_dir`. One name now, the old one still answered.
            "list_dir" | "list_directory" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                // Default true, because a hidden file is usually exactly what is being
                // looked for — the same default the in-app tool documents.
                let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(true);
                let head_limit = args
                    .get("head_limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .clamp(1, 2000);
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                if let Err(e) = self.allow_read(Some(&vps_id), path) {
                    return (format!("error: {e}"), true);
                }
                let flags = if all { "-la" } else { "-l" };
                let cmd = format!(
                    "ls {flags} --time-style=iso -- {} | head -n {head_limit}",
                    shell_quote(path)
                );
                match run_vps_command(&self.db, &vps_id, &cmd).await {
                    Ok(out) if out.exit_code == 0 => (truncate_output(&out.stdout), false),
                    Ok(out) => (format!("exit_code: {}\nstderr:\n{}", out.exit_code, out.stderr), true),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "grep_search" => {
                let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p,
                    _ => return ("error: missing pattern".into(), true),
                };
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                // The in-app schema spells this `case_insensitive` and bounds results
                // with `head_limit`. Both are read here so the advertised names work,
                // and the two names this transport used to advertise on its own still
                // do, for an agent mid-session that learned them.
                let case_sensitive = match args.get("case_insensitive").and_then(|v| v.as_bool()) {
                    Some(insensitive) => !insensitive,
                    None => args.get("case_sensitive").and_then(|v| v.as_bool()).unwrap_or(true),
                };
                let max_results = args
                    .get("head_limit")
                    .or_else(|| args.get("max_results"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .clamp(1, 250);
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                if let Err(e) = self.allow_read(Some(&vps_id), path) {
                    return (format!("error: {e}"), true);
                }
                let case_flag = if case_sensitive { "" } else { "-i" };
                let cmd = format!(
                    "grep -rn {case_flag} -m {max_results} --exclude-dir='.git' --exclude-dir='node_modules' --exclude-dir='target' -- {} {} 2>/dev/null | head -n {max_results}",
                    shell_quote(pattern),
                    shell_quote(path)
                );
                match run_vps_command(&self.db, &vps_id, &cmd).await {
                    Ok(out) => {
                        let res = if out.stdout.trim().is_empty() {
                            format!("(no matches found for '{pattern}' in '{path}')")
                        } else {
                            out.stdout
                        };
                        (truncate_output(&res), false)
                    }
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            // `file_search` was this transport's own name for `find_files`.
            "find_files" | "file_search" => {
                let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p,
                    _ => return ("error: missing pattern".into(), true),
                };
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(8);
                let head_limit = args
                    .get("head_limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(200)
                    .clamp(1, 2000);
                let kind_flag = match args.get("type").and_then(|v| v.as_str()).unwrap_or("file") {
                    "dir" => "-type d ",
                    "any" => "",
                    _ => "-type f ",
                };
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                if let Err(e) = self.allow_read(Some(&vps_id), path) {
                    return (format!("error: {e}"), true);
                }
                let cmd = format!(
                    "find {} -maxdepth {max_depth} {kind_flag}-name {} ! -path '*/.git/*' ! -path '*/node_modules/*' ! -path '*/target/*' 2>/dev/null | head -n {head_limit}",
                    shell_quote(path),
                    shell_quote(pattern)
                );
                match run_vps_command(&self.db, &vps_id, &cmd).await {
                    Ok(out) => {
                        let res = if out.stdout.trim().is_empty() {
                            format!("(no files matched '{pattern}' in '{path}')")
                        } else {
                            out.stdout
                        };
                        (truncate_output(&res), false)
                    }
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
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if name.trim().is_empty() || content.trim().is_empty() {
                    return ("error: name and non-empty content are required".into(), true);
                }
                let report = match crate::ai::skill_scan::scan_skill_content(
                    content,
                    &crate::ai::skill_scan::scan_options_from_db(&self.db),
                )
                .await
                {
                    Ok(report) => report,
                    Err(e) => return (format!("error: scanning skill: {e}"), true),
                };
                if report.is_blocking() {
                    return (format!("BLOCKED: skill was not saved.\n{}", report.summary()), true);
                }
                if self.effective_safety(None) != "full" {
                    return ("error: skill_save requires Full autonomy in xConsole MCP; review and save skills from Settings or the in-app agent".into(), true);
                }
                match skills::save_unverified(&self.home, name, content) {
                    Ok(saved) => (format!("saved unverified skill unverified/{saved}; promote it after review.\n{}", report.summary()), false),
                    Err(e) => (format!("error: {e}"), true),
                }
            }
            "memory_save" => {
                let entry = args.get("entry").and_then(|v| v.as_str()).unwrap_or("");
                if entry.trim().is_empty() {
                    return ("error: missing entry".into(), true);
                }
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
            "host_memory_get" => {
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                let profile = crate::ai::host_memory::load_profile(&self.home, &vps_id);
                let memory = crate::ai::host_memory::load_memory(&self.home, &vps_id);
                if profile.trim().is_empty() && memory.trim().is_empty() {
                    return (format!("(no dossier yet for {vps_id} — update with host_memory_update)"), false);
                }
                let mut out = String::new();
                if !profile.trim().is_empty() {
                    out.push_str("# PROFILE\n");
                    out.push_str(profile.trim());
                    out.push('\n');
                }
                if !memory.trim().is_empty() {
                    out.push_str("\n# MEMORY\n");
                    out.push_str(memory.trim());
                }
                (out, false)
            }
            "host_memory_update" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.trim().is_empty() {
                    return ("error: missing content".into(), true);
                }
                let vps_id = match self.resolve_vps(args) {
                    Ok(id) => id,
                    Err(e) => return (format!("error: {e}"), true),
                };
                match args.get("kind").and_then(|v| v.as_str()).unwrap_or("") {
                    "profile" => match crate::ai::host_memory::save_profile(&self.home, &vps_id, content) {
                        Ok(()) => ("saved host PROFILE".into(), false),
                        Err(e) => (format!("error: {e}"), true),
                    },
                    "memory" => match crate::ai::host_memory::append_memory(&self.home, &vps_id, content) {
                        Ok(_) => ("appended to host MEMORY".into(), false),
                        Err(e) => (format!("error: {e}"), true),
                    },
                    _ => ("error: kind must be 'profile' or 'memory'".into(), true),
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
    "command blocked: xConsole safety is Approve mode; switch to Full or Allowlist in xConsole Settings";

pub async fn run_stdio_server() -> Result<(), String> {
    let session = Arc::new(McpSession::from_env()?);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Dedicated asynchronous stdout response pump
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(msg) = rx.recv().await {
            if stdout.write_all(msg.as_bytes()).await.is_err() {
                break;
            }
            if stdout.write_all(b"\n").await.is_err() {
                break;
            }
            if stdout.flush().await.is_err() {
                break;
            }
        }
    });

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = json_response(
                    Value::Null,
                    None,
                    Some(json!({ "code": -32700, "message": format!("parse error: {e}") })),
                );
                let _ = tx.send(resp);
                continue;
            }
        };

        let method = match msg.get("method").and_then(|m| m.as_str()) {
            Some(m) => m.to_string(),
            None => continue,
        };

        // Client-driven cancellation
        if method == "notifications/cancelled" {
            if let Some(req_id) = msg
                .get("params")
                .and_then(|p| p.get("requestId"))
                .map(|r| r.to_string())
            {
                if let Some((_, handle)) = session.abort_handles.remove(&req_id) {
                    handle.abort();
                }
            }
            continue;
        }

        if method.starts_with("notifications/") {
            continue;
        }

        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let id_str = id.to_string();
        let session_clone = session.clone();
        let tx_clone = tx.clone();
        let task_id = id_str.clone();

        let join_handle = tokio::spawn(async move {
            let resp = dispatch_message(&session_clone, &method, &id, &msg).await;
            let _ = tx_clone.send(resp);
            session_clone.abort_handles.remove(&task_id);
        });

        session
            .abort_handles
            .insert(id_str, join_handle.abort_handle());
    }

    Ok(())
}

pub(crate) async fn dispatch_message(
    session: &Arc<McpSession>,
    method: &str,
    id: &Value,
    msg: &Value,
) -> String {
    // Optional Bearer Token Auth verification (when configured for reverse tunnels)
    if let Some(ref required_token) = session.token {
        let header_token = msg
            .get("params")
            .and_then(|p| p.get("_meta"))
            .and_then(|m| m.get("token"))
            .and_then(|t| t.as_str())
            .or_else(|| {
                msg.get("params")
                    .and_then(|p| p.get("token"))
                    .and_then(|t| t.as_str())
            });

        if header_token != Some(required_token.as_str()) && method != "initialize" && method != "ping" {
            return json_response(
                id.clone(),
                None,
                Some(json!({ "code": -32001, "message": "unauthorized: invalid or missing xConsole session token" })),
            );
        }
    }

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false },
                "logging": {}
            },
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
            let (text, is_error) = session.tool_call(name, &args).await;
            json!({
                "content": [{ "type": "text", "text": text }],
                "isError": is_error
            })
        }
        "resources/list" => session.resource_list(),
        "resources/templates/list" => session.resource_templates(),
        "resources/read" => {
            let uri = msg
                .get("params")
                .and_then(|p| p.get("uri"))
                .and_then(|u| u.as_str())
                .unwrap_or("");
            let (res, is_err) = session.resource_read(uri).await;
            if is_err {
                return json_response(
                    id.clone(),
                    None,
                    Some(json!({ "code": -32602, "message": res.get("error").and_then(|e| e.as_str()).unwrap_or("resource read failed") })),
                );
            }
            res
        }
        "prompts/list" => session.prompt_list(),
        "prompts/get" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match session.prompt_get(name, &args) {
                Ok(p) => p,
                Err(e) => {
                    return json_response(
                        id.clone(),
                        None,
                        Some(json!({ "code": -32602, "message": e })),
                    );
                }
            }
        }
        "logging/setLevel" => json!({}),
        "ping" => json!({}),
        _ => {
            return json_response(
                id.clone(),
                None,
                Some(json!({ "code": -32601, "message": format!("method not found: {method}") })),
            );
        }
    };

    json_response(id.clone(), Some(result), None)
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

#[cfg(test)]
mod bridged_tool_tests {
    use super::*;

    fn shared() -> Vec<crate::ai::provider::ToolDef> {
        // `definitions` ignores the home it is handed; it only needs one to exist.
        crate::ai::tools::definitions(&crate::ai::AgentHome::new(std::env::temp_dir()))
    }

    #[test]
    fn every_bridged_tool_takes_its_description_from_the_shared_definition() {
        // The bug this replaces: a second, hand-written copy of these schemas lived
        // here, and it was the copy a Claude Code agent on the VPS actually read. A
        // name that no longer resolves means the bridge has silently gone back to
        // advertising nothing for that tool.
        let defs = shared();
        for b in BRIDGED {
            assert!(
                defs.iter().any(|d| d.name == b.name),
                "{} is bridged but no longer exists in tools::definitions()",
                b.name
            );
        }
    }

    #[test]
    fn no_tool_advertises_an_argument_this_transport_ignores() {
        // Advertising a parameter that is silently dropped is how a model concludes a
        // tool is broken and stops using it.
        let defs = shared();
        for b in BRIDGED {
            let def = defs.iter().find(|d| d.name == b.name).unwrap();
            let schema = bridged_schema(&def.parameters, b);
            let props = schema["properties"].as_object().unwrap();
            for key in props.keys() {
                assert!(
                    b.props.contains(&key.as_str()),
                    "{} advertises {key}, which its handler never reads",
                    b.name
                );
            }
        }
    }

    #[test]
    fn a_tool_that_cannot_work_without_an_argument_declares_it_required() {
        // `read_file` is the one that matters: the shared schema declares no `required`
        // at all, which reads as "every argument is optional", and a read_file with no
        // path is a parse failure waiting to happen.
        let defs = shared();
        let must = [
            ("run_command", "command"),
            ("read_file", "path"),
            ("write_file", "content"),
            ("edit_file", "old_string"),
            ("grep_search", "pattern"),
            ("find_files", "pattern"),
        ];
        for (name, key) in must {
            let b = BRIDGED.iter().find(|b| b.name == name).unwrap();
            let def = defs.iter().find(|d| d.name == name).unwrap();
            let schema = bridged_schema(&def.parameters, b);
            let required = schema["required"].as_array().unwrap();
            assert!(
                required.iter().any(|v| v == key),
                "{name} does not require {key}"
            );
        }
    }

    #[test]
    fn reading_a_file_is_one_tool_with_an_optional_window() {
        // Two entries meant the unbounded one was the obvious call and the bounded one
        // was the exception, so a 200 MB log was one plausible tool call away.
        assert!(
            !BRIDGED.iter().any(|b| b.name == "read_file_range"),
            "read_file_range must not be advertised as a separate tool"
        );
        let b = BRIDGED.iter().find(|b| b.name == "read_file").unwrap();
        assert!(b.props.contains(&"offset"));
        assert!(b.props.contains(&"limit"));
    }

    #[test]
    fn a_note_is_added_only_where_this_transport_really_differs() {
        // run_command has no job runner here, and a description that promised one would
        // send the agent looking for job_status on a connection that has none.
        let defs = shared();
        let b = BRIDGED.iter().find(|b| b.name == "run_command").unwrap();
        let def = defs.iter().find(|d| d.name == "run_command").unwrap();
        let text = bridged_description(&def.description, b.note);
        assert!(text.contains("fresh shell"), "the shared prose must survive: {text}");
        assert!(text.contains("On this connection:"), "{text}");
        assert!(text.contains("nohup"), "it must say what to do instead: {text}");

        // And nothing is appended where there is no difference.
        let b = BRIDGED.iter().find(|b| b.name == "list_dir").unwrap();
        let def = defs.iter().find(|d| d.name == "list_dir").unwrap();
        assert!(!bridged_description(&def.description, b.note).contains("On this connection:"));
    }
}
