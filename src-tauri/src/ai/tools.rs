//! Agent tools and dispatch. One dispatch function maps a tool call to an action
//! (SSH command, file read/write, memory, skills) and applies the safety gate.

use base64::Engine;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::ai::infra_tools;
use crate::ai::web_tools;
use crate::ai::provider::{emit, EventSink, StreamEvent, ToolCall, ToolDef, ActivityEvent};
use crate::ai::safety::{self, ApprovalRegistry};
use crate::ai::{memory, skills, AgentHome};
use crate::ssh::SessionManager;
use crate::storage::Db;

/// Everything a tool needs to run. Holds owned clones (all cheap to clone).
pub struct ToolContext {
    pub app: AppHandle,
    pub db: Db,
    pub sessions: SessionManager,
    pub home: AgentHome,
    pub approvals: ApprovalRegistry,
    pub session_id: String,
    /// VPS ids the agent may act on this turn.
    pub targets: Vec<String>,
    pub safety: String,
}

/// Tool schemas advertised to the model.
pub fn definitions(_home: &AgentHome) -> Vec<ToolDef> {
    let mut defs = vec![
        ToolDef {
            name: "run_command".into(),
            description: "Run a shell command on one server over SSH. When multiple targets are \
selected, vps_id is required (exact UUID from the target list).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to run."},
                    "vps_id": {"type": "string", "description": "Exact target UUID. Required when more than one VPS is selected."}
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "run_command_all".into(),
            description: "Run the same shell command on every selected VPS target and return \
combined output. Prefer this when the user asks about both/all/each server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to run on each target."}
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "list_vps_targets".into(),
            description: "List the VPS targets selected for this session with exact vps_id UUIDs.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "read_file".into(),
            description: "Read a text file from a server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "vps_id": {"type": "string"}
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "write_file".into(),
            description: "Write (overwrite) a text file on a server. Subject to the safety mode. \
Use /root/ or /tmp/ on Linux (root login) — not /home/root/. Prefer hello.py over names with spaces."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "vps_id": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "memory_save".into(),
            description: "Save a durable, reusable fact to persistent memory (MEMORY.md). Keep it terse."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {"entry": {"type": "string"}},
                "required": ["entry"]
            }),
        },
        ToolDef {
            name: "skills_list".into(),
            description: "List available skills (reusable playbooks) by category.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "skill_view".into(),
            description: "Read the full SKILL.md for a skill before using it.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string"},
                    "name": {"type": "string"}
                },
                "required": ["category", "name"]
            }),
        },
        ToolDef {
            name: "skill_save".into(),
            description: "Create or update a reusable skill (a SKILL.md playbook) under a category."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string"},
                    "name": {"type": "string"},
                    "content": {"type": "string", "description": "Full SKILL.md markdown."}
                },
                "required": ["category", "name", "content"]
            }),
        },
    ];
    defs.extend(web_tools::definitions());
    defs.extend(infra_tools::definitions());
    defs
}

const OLLAMA_VPS_TOOLS: &[&str] = &[
    "run_command",
    "run_command_all",
    "list_vps_targets",
    "read_file",
    "write_file",
    "memory_save",
    "skills_list",
    "skill_view",
    "skill_save",
];

const OLLAMA_LOCAL_TOOLS: &[&str] = &["memory_save", "skills_list", "skill_view", "skill_save"];

/// Tool schemas for local Ollama — always includes web; VPS tools when targets are set.
pub fn definitions_for_ollama(home: &AgentHome, target_count: usize, casual: bool) -> Vec<ToolDef> {
    let mut defs = web_tools::definitions();
    if casual {
        return defs;
    }
    let extra_names: &[&str] = if target_count > 0 {
        OLLAMA_VPS_TOOLS
    } else {
        OLLAMA_LOCAL_TOOLS
    };
    defs.extend(
        definitions(home)
            .into_iter()
            .filter(|t| extra_names.contains(&t.name.as_str())),
    );
    defs
}

/// Exact target ids + hostnames for the system prompt and list_vps_targets.
pub fn format_targets_catalog(db: &Db, target_ids: &[String]) -> String {
    if target_ids.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "# Selected VPS targets (use these exact vps_id UUIDs — not 0, 1, or hostnames)".to_string(),
    ];
    for id in target_ids {
        match db.get_vps(id) {
            Ok(Some(vps)) => lines.push(format!(
                "- vps_id `{id}`: {} — {}@{}:{}",
                vps.name, vps.username, vps.host, vps.port
            )),
            _ => lines.push(format!("- vps_id `{id}`: (not found in database)")),
        }
    }
    if target_ids.len() > 1 {
        lines.push(
            "When the user asks about both/all servers, use run_command_all instead of run_command."
                .into(),
        );
    }
    lines.join("\n")
}

/// Run a single tool call, returning a text result for the model.
pub async fn dispatch(ctx: &ToolContext, call: &ToolCall, sink: &EventSink) -> String {
    let label = tool_activity_label(ctx, call);
    emit(
        Some(sink),
        StreamEvent::Activity(ActivityEvent::ToolStart {
            id: call.id.clone(),
            tool: call.name.clone(),
            label: label.clone(),
            detail: None,
        }),
    );
    emit_skill_activity(ctx, call, sink);

    let args = &call.arguments;
    let result = match call.name.as_str() {
        "run_command" => run_command(ctx, args, sink, &call.id).await,
        "run_command_all" => run_command_all(ctx, args, sink, &call.id).await,
        "list_vps_targets" => list_vps_targets(ctx),
        "read_file" => read_file(ctx, args, sink, &call.id).await,
        "write_file" => write_file(ctx, args, sink, &call.id).await,
        "memory_save" => memory_save(ctx, args),
        "skills_list" => skills_list(ctx),
        "skill_view" => skill_view(ctx, args),
        "skill_save" => skill_save(ctx, args),
        name if web_tools::is_web_tool(name) => web_tools::dispatch(name, args).await,
        name if name.starts_with("project_")
            || name.starts_with("terraform_")
            || name.starts_with("cloud_")
            || name.starts_with("tfc_") =>
        {
            infra_tools::dispatch(ctx, call.name.as_str(), args, sink).await
        }
        other => format!("error: unknown tool '{other}'"),
    };

    let ok = !result.starts_with("error:");
    emit(
        Some(sink),
        StreamEvent::Activity(ActivityEvent::ToolEnd {
            id: call.id.clone(),
            ok,
        }),
    );
    result
}

fn tool_activity_label(ctx: &ToolContext, call: &ToolCall) -> String {
    let args = &call.arguments;
    match call.name.as_str() {
        "run_command" => {
            let cmd = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("…");
            let vps = vps_label(ctx, args);
            format!("Run on {vps}: {cmd}")
        }
        "run_command_all" => {
            let cmd = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("…");
            format!("Run on all {} targets: {cmd}", ctx.targets.len())
        }
        "list_vps_targets" => "List VPS targets".into(),
        "read_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Read {} on {}", path, vps_label(ctx, args))
        }
        "write_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Write {} on {}", path, vps_label(ctx, args))
        }
        "memory_save" => "Save to memory".into(),
        "skills_list" => "List skills".into(),
        "skill_view" => {
            let cat = args.get("category").and_then(|v| v.as_str()).unwrap_or("?");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            format!("Read skill {cat}/{name}")
        }
        "skill_save" => {
            let cat = args.get("category").and_then(|v| v.as_str()).unwrap_or("?");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            format!("Save skill {cat}/{name}")
        }
        "web_search" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Web search · {q}")
        }
        "web_fetch" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Web fetch · {url}")
        }
        other => other.replace('_', " "),
    }
}

fn vps_label(ctx: &ToolContext, args: &Value) -> String {
    if let Some(id) = args.get("vps_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        if let Ok(Some(vps)) = ctx.db.get_vps(id) {
            return format!("{} ({})", vps.name, vps.host);
        }
        return id.to_string();
    }
    if ctx.targets.len() == 1 {
        if let Ok(Some(vps)) = ctx.db.get_vps(&ctx.targets[0]) {
            return format!("{} ({})", vps.name, vps.host);
        }
    }
    "selected VPS".into()
}

fn emit_skill_activity(ctx: &ToolContext, call: &ToolCall, sink: &EventSink) {
    let args = &call.arguments;
    match call.name.as_str() {
        "skill_view" => {
            let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            emit(
                Some(sink),
                StreamEvent::Activity(ActivityEvent::SkillRead {
                    id: call.id.clone(),
                    category: category.into(),
                    name: name.into(),
                }),
            );
        }
        "skill_save" => {
            let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            emit(
                Some(sink),
                StreamEvent::Activity(ActivityEvent::SkillSaved {
                    id: call.id.clone(),
                    category: category.into(),
                    name: name.into(),
                }),
            );
        }
        "run_command" => {
            if let Ok(vps_id) = resolve_target(ctx, args) {
                if let Ok(Some(vps)) = ctx.db.get_vps(&vps_id) {
                    let command = args
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    emit(
                        Some(sink),
                        StreamEvent::Activity(ActivityEvent::Command {
                            id: call.id.clone(),
                            vps: format!("{} ({})", vps.name, vps.host),
                            command,
                        }),
                    );
                }
            }
        }
        "run_command_all" => {}
        _ => {}
    }
}

/// Whether `vps_id` is within the user-selected target set for this turn.
pub fn is_target_allowed(allowed: &[String], vps_id: &str) -> bool {
    !allowed.is_empty() && allowed.iter().any(|t| t == vps_id)
}

fn target_scope_error(vps_id: &str, allowed: &[String]) -> String {
    format!(
        "vps_id '{vps_id}' is not in the selected targets (allowed: {})",
        allowed.join(", ")
    )
}

/// Resolve which VPS a tool should target. Explicit `vps_id` values must fall
/// within `ctx.targets`; at least one target must be selected.
pub fn resolve_target(ctx: &ToolContext, args: &Value) -> Result<String, String> {
    if ctx.targets.is_empty() {
        return Err("no VPS targets selected; ask the user to select a target or pass vps_id".into());
    }
    if let Some(id) = args.get("vps_id").and_then(|v| v.as_str()) {
        if !id.is_empty() {
            if !is_target_allowed(&ctx.targets, id) {
                // Allow 0-based index when the model passes "0" or "1" instead of UUID.
                if let Ok(idx) = id.parse::<usize>() {
                    if idx < ctx.targets.len() {
                        return Ok(ctx.targets[idx].clone());
                    }
                }
                return Err(target_scope_error(id, &ctx.targets));
            }
            return Ok(id.to_string());
        }
    }
    match ctx.targets.len() {
        1 => Ok(ctx.targets[0].clone()),
        _ => Err(format!(
            "multiple targets selected; pass vps_id (one of: {})",
            ctx.targets.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_allowed_only_when_in_list() {
        let allowed = vec!["a".into(), "b".into()];
        assert!(is_target_allowed(&allowed, "a"));
        assert!(!is_target_allowed(&allowed, "c"));
        assert!(!is_target_allowed(&[], "a"));
    }
}

/// Single-quote a string for safe POSIX shell interpolation.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Run a command on a specific VPS. Shared by tool dispatch and CLI VPS snapshots.
pub async fn exec_on_vps(
    ctx: &ToolContext,
    vps_id: &str,
    command: &str,
    sink: &EventSink,
) -> String {
    exec(ctx, vps_id, command, Some(sink), true).await
}

/// Same as `exec_on_vps` but does not emit `$ command` status (caller shows its own UI).
pub async fn exec_on_vps_quiet(
    ctx: &ToolContext,
    vps_id: &str,
    command: &str,
) -> String {
    exec(ctx, vps_id, command, None, false).await
}

fn emit_command_activity(
    ctx: &ToolContext,
    sink: &EventSink,
    activity_id: &str,
    vps_id: &str,
    command: &str,
) {
    let vps_label = ctx
        .db
        .get_vps(vps_id)
        .ok()
        .flatten()
        .map(|v| format!("{} ({})", v.name, v.host))
        .unwrap_or_else(|| vps_id.to_string());
    emit(
        Some(sink),
        StreamEvent::Activity(ActivityEvent::Command {
            id: activity_id.to_string(),
            vps: vps_label,
            command: command.to_string(),
        }),
    );
}

fn emit_command_result(sink: &EventSink, activity_id: &str, output: &str) {
    emit(
        Some(sink),
        StreamEvent::ToolResult {
            id: activity_id.to_string(),
            output: output.to_string(),
        },
    );
}

pub fn emit_command_activity_public(
    ctx: &ToolContext,
    sink: &EventSink,
    activity_id: &str,
    vps_id: &str,
    command: &str,
) {
    emit_command_activity(ctx, sink, activity_id, vps_id, command);
}

pub fn emit_command_result_public(sink: &EventSink, activity_id: &str, output: &str) {
    emit_command_result(sink, activity_id, output);
}

async fn exec_inner(ctx: &ToolContext, vps_id: &str, command: &str) -> String {
    let mode = safety::effective_mode(&ctx.db, &ctx.safety, vps_id);
    if let Err(e) = safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &mode,
        &ctx.session_id,
        Some(vps_id),
        command,
    )
    .await
    {
        return format!("error: {e}");
    }

    match ctx.sessions.run_command(vps_id, command).await {
        Ok(out) => {
            let mut s = format!("exit_code: {}\n", out.exit_code);
            if out.exit_code == -1 && !out.stdout.trim().is_empty() {
                s.push_str(
                    "note: SSH channel closed without exit status; stdout below is still valid.\n",
                );
            }
            if !out.stdout.is_empty() {
                s.push_str(&format!("stdout:\n{}\n", out.stdout.trim_end()));
            }
            if !out.stderr.is_empty() {
                s.push_str(&format!("stderr:\n{}\n", out.stderr.trim_end()));
            }
            s
        }
        Err(e) => format!("error running command: {e}"),
    }
}

async fn exec(
    ctx: &ToolContext,
    vps_id: &str,
    command: &str,
    sink: Option<&EventSink>,
    emit_command: bool,
) -> String {
    let activity_id = format!("cmd-{vps_id}");
    if emit_command {
        if let Some(s) = sink {
            emit_command_activity(ctx, s, &activity_id, vps_id, command);
        }
    }
    let mut result = exec_inner(ctx, vps_id, command).await;
    result = crate::ai::vps_snapshot::annotate_command_output(command, &result);
    if emit_command {
        if let Some(s) = sink {
            emit_command_result(s, &activity_id, &result);
        }
    }
    result
}

async fn run_command(ctx: &ToolContext, args: &Value, sink: &EventSink, id: &str) -> String {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return "error: missing 'command'".into(),
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let mut result = exec_inner(ctx, &vps_id, command).await;
    result = crate::ai::vps_snapshot::annotate_command_output(command, &result);
    emit_command_result(sink, id, &result);
    result
}

async fn run_command_all(ctx: &ToolContext, args: &Value, sink: &EventSink, id: &str) -> String {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return "error: missing 'command'".into(),
    };
    run_command_all_targets_impl(ctx, command, sink, id).await
}

/// Run one command on every selected target (shared by tools and Ollama auto-collect).
pub async fn run_command_all_targets(
    ctx: &ToolContext,
    command: &str,
    sink: &EventSink,
) -> String {
    run_command_all_targets_impl(ctx, command, sink, "auto").await
}

async fn run_command_all_targets_impl(
    ctx: &ToolContext,
    command: &str,
    sink: &EventSink,
    activity_prefix: &str,
) -> String {
    if ctx.targets.is_empty() {
        return "error: no VPS targets selected".into();
    }
    let mut parts: Vec<String> = Vec::with_capacity(ctx.targets.len());
    for (i, vps_id) in ctx.targets.iter().enumerate() {
        let activity_id = format!("{activity_prefix}-{vps_id}-{i}");
        emit_command_activity(ctx, sink, &activity_id, vps_id, command);

        let header = match ctx.db.get_vps(vps_id) {
            Ok(Some(vps)) => format!(
                "=== {} (`{vps_id}`) — {}@{}:{} ===",
                vps.name, vps.username, vps.host, vps.port
            ),
            _ => format!("=== `{vps_id}` ==="),
        };
        let mut out = exec_inner(ctx, vps_id, command).await;
        out = crate::ai::vps_snapshot::annotate_command_output(command, &out);
        emit_command_result(sink, &activity_id, &out);
        parts.push(format!("{header}\n{out}"));
    }
    if ctx.targets.len() == 1 {
        parts.push(
            "note: ran on 1 selected target only. \
             Select additional VPS targets in the agent panel to include more servers."
                .to_string(),
        );
    } else {
        parts.push(format!(
            "note: ran on {} selected target(s). Summarize every === section above.",
            ctx.targets.len()
        ));
    }
    parts.join("\n\n")
}

fn list_vps_targets(ctx: &ToolContext) -> String {
    format_targets_catalog(&ctx.db, &ctx.targets)
}

async fn read_file(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let command = format!("cat -- {}", shell_quote(path));
    exec(ctx, &vps_id, &command, Some(sink), true).await
}

async fn write_file(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => normalize_vps_write_path(p),
        _ => return "error: missing 'path'".into(),
    };
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let parent = std::path::Path::new(&path)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|p| !p.is_empty())
        .unwrap_or("/tmp");
    // Transfer via base64 to avoid any quoting/encoding issues.
    let b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        shell_quote(parent),
        shell_quote(&b64),
        shell_quote(&path)
    );
    exec(ctx, &vps_id, &command, Some(sink), true).await
}

/// Fix common bad paths from local models (e.g. /home/root/ → /root/).
fn normalize_vps_write_path(path: &str) -> String {
    let mut p = path.trim().to_string();
    if p.starts_with("/home/root/") {
        p = p.replacen("/home/root/", "/root/", 1);
    } else if p == "/home/root" {
        p = "/root".into();
    }
    p
}

fn memory_save(ctx: &ToolContext, args: &Value) -> String {
    let entry = args.get("entry").and_then(|v| v.as_str()).unwrap_or("");
    if entry.trim().is_empty() {
        return "error: missing 'entry'".into();
    }
    match memory::append_memory(&ctx.home, entry) {
        Ok(_) => "saved to memory".into(),
        Err(e) => format!("error: {e}"),
    }
}

fn skills_list(ctx: &ToolContext) -> String {
    let skills = skills::discover(&ctx.home);
    if skills.is_empty() {
        return "no skills installed".into();
    }
    skills
        .iter()
        .map(|s| format!("{}/{} — {}", s.category, s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n")
}

fn skill_view(ctx: &ToolContext, args: &Value) -> String {
    let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    match skills::read_skill(&ctx.home, category, name) {
        Some(body) => body,
        None => format!("error: skill '{category}/{name}' not found"),
    }
}

fn skill_save(ctx: &ToolContext, args: &Value) -> String {
    let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    match skills::save_skill(&ctx.home, category, name, content) {
        Ok(()) => format!("saved skill {category}/{name}"),
        Err(e) => format!("error: {e}"),
    }
}
