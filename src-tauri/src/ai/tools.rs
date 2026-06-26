//! Agent tools and dispatch. One dispatch function maps a tool call to an action
//! (SSH command, file read/write, memory, skills) and applies the safety gate.

use base64::Engine;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::ai::infra_tools;
use crate::ai::web_tools;
use crate::ai::provider::{emit, EventSink, StreamEvent, ToolCall, ToolDef, ActivityEvent};
use crate::ai::interaction::{PromptRegistry, SessionState};
use crate::ai::safety::{self, ApprovalRegistry};
use crate::ai::{hooks, memory, skill_install, skill_scan, skills, workspace_context, AgentHome};
use crate::secrets;
use crate::ssh::{keygen, shell_quote, SessionManager};
use crate::storage::Db;
use tauri::Emitter;
use uuid::Uuid;

/// How long an interactive prompt (ask_user / present_plan) waits for the user
/// before giving up. Generous — plans and questions can take a while to answer.
const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1800);

/// Everything a tool needs to run. Holds owned clones (all cheap to clone).
pub struct ToolContext {
    pub app: AppHandle,
    pub db: Db,
    pub sessions: SessionManager,
    pub home: AgentHome,
    pub approvals: ApprovalRegistry,
    /// Registry for blocking interactive prompts (ask_user / present_plan).
    pub prompts: PromptRegistry,
    /// Per-session flags (safety override, plan-approved).
    pub session_state: SessionState,
    pub session_id: String,
    /// VPS ids the agent may act on this turn.
    pub targets: Vec<String>,
    pub safety: String,
    /// Plan mode: the agent must present an approved plan before mutating anything.
    pub plan_mode: bool,
    /// Active workspace id, if any — scopes project brief/memory and project files.
    pub workspace_id: Option<String>,
    /// Live canvas snapshot reported by the frontend: the terminals / SFTP panels
    /// the user currently has open (so the agent can see and act on them).
    pub canvas: Vec<crate::ai::canvas_context::CanvasNode>,
    /// Journal of files the agent edits this session (for the diff/changes panel).
    pub edits: crate::ai::edits::EditJournal,
    /// Claude Code–style lifecycle hooks (snapshotted at startup). Empty = disabled.
    pub hooks: crate::ai::hooks::HooksConfig,
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
            description: "Save a durable, reusable fact to persistent memory. Keep it terse. When a \
workspace/project is active this saves to that workspace's memory; otherwise to global memory."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {"entry": {"type": "string"}},
                "required": ["entry"]
            }),
        },
        ToolDef {
            name: "set_project_brief".into(),
            description: "Create or update the brief for the active workspace's project — what it is, \
its layout, conventions, and what the user is working on. The brief is shown to you whenever this \
workspace is active. Keep it current as you learn more. Requires an active workspace."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "Full brief as markdown."}
                },
                "required": ["content"]
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
        ToolDef {
            name: "skill_install".into(),
            description: "Download and install a skill (a SKILL.md playbook) from a URL or GitHub folder \
so your abilities can grow. Every skill is security-scanned before install: a failing scan is blocked, \
and installs from sources other than the official Anthropic repo require user approval. Source examples: \
a GitHub folder URL (https://github.com/anthropics/skills/tree/main/pdf) or a raw SKILL.md URL."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string", "description": "URL to a SKILL.md or a skill folder."},
                    "category": {"type": "string", "description": "Optional category (default 'downloaded')."},
                    "name": {"type": "string", "description": "Optional skill name (default derived from the URL)."}
                },
                "required": ["source"]
            }),
        },
        ToolDef {
            name: "list_official_skills".into(),
            description: "List the skills available in Anthropic's official skills repository so you can \
pick one to install with skill_install.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "learn_skill".into(),
            description: "Research an unfamiliar tool, API, error, or procedure on the web and build a \
reusable skill, then return it so you can apply it right now. Use this instead of stating commands, \
flags, or steps from memory when you're not certain — it learns the capability for you.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "topic": {"type": "string", "description": "The capability to learn, as a generic phrase (no private hostnames/IPs/secrets), e.g. 'configure ufw firewall on ubuntu'."},
                    "name": {"type": "string", "description": "Optional skill name (derived from the topic if omitted)."}
                },
                "required": ["topic"]
            }),
        },
        ToolDef {
            name: "local_run_command".into(),
            description: "Run a shell command on the user's LOCAL machine (this PC), not a remote \
server. Use this when the user says 'my pc', 'locally', 'this machine', or asks to check local \
software (e.g. local docker containers). On Windows the command runs in PowerShell; on macOS/Linux \
in sh. For remote servers use run_command instead.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to run on this PC."}
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "local_read_file".into(),
            description: "Read a text file from the user's local machine (this PC).".into(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "local_write_file".into(),
            description: "Write (overwrite) a text file on the user's local machine (this PC). \
Parent directories are created automatically. Subject to the safety mode.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "local_list_dir".into(),
            description: "List the contents of a directory on the user's local machine (this PC)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "upload_file".into(),
            description: "Upload a file from the user's local machine to a server over SSH \
(binary-safe). When multiple targets are selected, vps_id is required.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "local_path": {"type": "string", "description": "Source path on this PC."},
                    "remote_path": {"type": "string", "description": "Absolute destination path on the server."},
                    "vps_id": {"type": "string"}
                },
                "required": ["local_path", "remote_path"]
            }),
        },
        ToolDef {
            name: "download_file".into(),
            description: "Download a file from a server to the user's local machine over SSH \
(binary-safe, up to 10 MB). When multiple targets are selected, vps_id is required.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "remote_path": {"type": "string", "description": "Absolute source path on the server."},
                    "local_path": {"type": "string", "description": "Destination path on this PC."},
                    "vps_id": {"type": "string"}
                },
                "required": ["remote_path", "local_path"]
            }),
        },
        ToolDef {
            name: "ssh_setup_key_auth".into(),
            description: "Switch a server from password login to a secure app-managed SSH key: \
generate an Ed25519 keypair, install the public key in the server's authorized_keys, store the \
private key in the OS keychain, and verify key login works. Password login on the server is left \
enabled (no lockout). Use this when the user wants key-based auth instead of passwords.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"vps_id": {"type": "string"}},
                "required": []
            }),
        },
        ToolDef {
            name: "ssh_key_status".into(),
            description: "Report how a server authenticates (password / key file / app-managed key) \
and the managed key fingerprint if present.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"vps_id": {"type": "string"}},
                "required": []
            }),
        },
        ToolDef {
            name: "ask_user".into(),
            description: "Ask the user one or more clarifying questions before proceeding, when the \
request is ambiguous or you need a decision only they can make. Each question may offer suggested \
options (rendered as buttons); the user can also type their own answer. Use this instead of guessing. \
Blocks until the user answers."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "description": "One or more questions to ask.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {"type": "string", "description": "The question text."},
                                "header": {"type": "string", "description": "Short label (a few words)."},
                                "options": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "Optional suggested answers, shown as buttons."
                                },
                                "multi": {"type": "boolean", "description": "Allow selecting multiple options."}
                            },
                            "required": ["question"]
                        }
                    }
                },
                "required": ["questions"]
            }),
        },
        ToolDef {
            name: "present_plan".into(),
            description: "Present a step-by-step plan to the user and wait for approval BEFORE making \
any changes. Use this for large, multi-step, or destructive tasks (and always when plan mode is on): \
first investigate with read-only tools, then call present_plan. The user can approve the plan (you \
then execute it) or request changes (you revise and present again). Blocks until the user responds."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Short title for the plan."},
                    "plan": {"type": "string", "description": "The full plan as markdown (numbered steps)."}
                },
                "required": ["plan"]
            }),
        },
        ToolDef {
            name: "terminal_send".into(),
            description: "Type into the LIVE terminal the user has open on the canvas for a server \
(so they watch you work), then optionally press Enter. Use this to drive the visible terminal; for a \
private one-off command prefer run_command. Requires an open terminal for that server."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Text/keys to type."},
                    "submit": {"type": "boolean", "description": "Press Enter after (default true)."},
                    "vps_id": {"type": "string"}
                },
                "required": ["text"]
            }),
        },
        ToolDef {
            name: "terminal_capture".into(),
            description: "Read the recent on-screen text (scrollback) of the user's live terminal for a \
server, to see the result of what you typed.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"vps_id": {"type": "string"}},
                "required": []
            }),
        },
        ToolDef {
            name: "canvas_open_terminal".into(),
            description: "Open a terminal for a server on the canvas (so the user can watch it).".into(),
            parameters: json!({
                "type": "object",
                "properties": {"vps_id": {"type": "string"}},
                "required": []
            }),
        },
        ToolDef {
            name: "canvas_open_sftp".into(),
            description: "Open an SFTP file browser for a server on the canvas.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"vps_id": {"type": "string"}},
                "required": []
            }),
        },
        ToolDef {
            name: "canvas_tile".into(),
            description: "Arrange the open canvas terminals/SFTP panels into a grid that fills the window."
                .into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "canvas_close".into(),
            description: "Close (remove from the canvas) a panel. Pass node_id to close one specific \
                          panel (from the Live canvas list); otherwise pass vps_id to close every \
                          terminal/SFTP node for that server."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "node_id": {"type": "string", "description": "Close just this panel (preferred)."},
                    "vps_id": {"type": "string", "description": "Close all panels for this server."}
                },
                "required": []
            }),
        },
        ToolDef {
            name: "canvas_refresh".into(),
            description: "Reconnect a terminal on the canvas (e.g. after the server rebooted and the \
                          console went disconnected). Pass node_id for one terminal, or vps_id to \
                          reconnect every terminal for that server."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "node_id": {"type": "string"},
                    "vps_id": {"type": "string"}
                },
                "required": []
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
    "upload_file",
    "download_file",
    "ssh_setup_key_auth",
    "ssh_key_status",
    "local_run_command",
    "local_read_file",
    "local_write_file",
    "local_list_dir",
    "terminal_send",
    "terminal_capture",
    "canvas_open_terminal",
    "canvas_open_sftp",
    "canvas_tile",
    "canvas_close",
    "canvas_refresh",
    "ask_user",
    "present_plan",
    "memory_save",
    "set_project_brief",
    "skills_list",
    "skill_view",
    "skill_save",
    "skill_install",
    "list_official_skills",
    "learn_skill",
];

// Even with no VPS target selected, the agent can still act on the local PC.
const OLLAMA_LOCAL_TOOLS: &[&str] = &[
    "local_run_command",
    "local_read_file",
    "local_write_file",
    "local_list_dir",
    "ask_user",
    "present_plan",
    "memory_save",
    "set_project_brief",
    "skills_list",
    "skill_view",
    "skill_save",
    "skill_install",
    "list_official_skills",
    "learn_skill",
];

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

    // PreToolUse hooks: a user-configured command can block this tool before it runs
    // (exit 2 / `decision:block` / `permissionDecision:deny`) or inject extra context
    // for the model. Fires only when something subscribes to PreToolUse (zero cost
    // otherwise). See `ai::hooks`.
    let mut hook_notes: Vec<String> = Vec::new();
    if ctx.hooks.has_event(hooks::HookEvent::PreToolUse) {
        let cwd = hooks::cwd();
        let input = hooks::HookEventInput {
            event: hooks::HookEvent::PreToolUse,
            session_id: &ctx.session_id,
            cwd: &cwd,
            workspace_id: ctx.workspace_id.as_deref(),
            vps_targets: &ctx.targets,
            tool_name: Some(&call.name),
            tool_input: Some(&call.arguments),
            tool_response: None,
            prompt: None,
        };
        let decision = hooks::run_event(&ctx.hooks, &input).await;
        if let Some(msg) = &decision.system_message {
            emit(Some(sink), StreamEvent::Status(msg.clone()));
        }
        if decision.blocks() {
            let reason = decision
                .reason
                .unwrap_or_else(|| "blocked by a PreToolUse hook".to_string());
            emit(
                Some(sink),
                StreamEvent::Activity(ActivityEvent::ToolEnd {
                    id: call.id.clone(),
                    ok: false,
                }),
            );
            return format!("error: blocked by hook: {reason}");
        }
        if let Some(extra) = decision.additional_context {
            hook_notes.push(format!("[PreToolUse hook] {extra}"));
        }
    }

    // Plan-mode guard: until the user approves a plan, block anything that would
    // change the PC or a server. Read-only inspection, ask_user, and present_plan
    // still run so the agent can investigate and propose its plan.
    let result = if ctx.plan_mode
        && !ctx.session_state.plan_approved(&ctx.session_id)
        && tool_is_mutating(&call.name, args)
    {
        format!(
            "error: plan mode is active. Investigate with read-only tools, then call present_plan \
             with your plan and wait for the user to approve it before running '{}'.",
            call.name
        )
    } else {
        match call.name.as_str() {
        "run_command" => run_command(ctx, args, sink, &call.id).await,
        "run_command_all" => run_command_all(ctx, args, sink, &call.id).await,
        "list_vps_targets" => list_vps_targets(ctx),
        "read_file" => read_file(ctx, args, sink, &call.id).await,
        "write_file" => write_file(ctx, args, sink, &call.id).await,
        "local_run_command" => local_run_command(ctx, args).await,
        "local_read_file" => local_read_file(ctx, args).await,
        "local_write_file" => local_write_file(ctx, args).await,
        "local_list_dir" => local_list_dir(ctx, args).await,
        "upload_file" => upload_file(ctx, args, sink, &call.id).await,
        "download_file" => download_file(ctx, args, sink, &call.id).await,
        "ssh_setup_key_auth" => ssh_setup_key_auth(ctx, args).await,
        "ssh_key_status" => ssh_key_status(ctx, args),
        "ask_user" => ask_user(ctx, args).await,
        "present_plan" => present_plan(ctx, args).await,
        "set_project_brief" => set_project_brief(ctx, args),
        "skill_install" => skill_install_tool(ctx, args).await,
        "list_official_skills" => skill_install::list_official_skills().await,
        "terminal_send" => terminal_send(ctx, args).await,
        "terminal_capture" => terminal_capture(ctx, args),
        "canvas_open_terminal" => canvas_command_tool(ctx, args, "open_terminal"),
        "canvas_open_sftp" => canvas_command_tool(ctx, args, "open_sftp"),
        "canvas_tile" => canvas_tile_tool(ctx),
        "canvas_close" => canvas_node_command(ctx, args, "close"),
        "canvas_refresh" => canvas_node_command(ctx, args, "reconnect"),
        "memory_save" => memory_save(ctx, args),
        "skills_list" => skills_list(ctx),
        "skill_view" => skill_view(ctx, args),
        "skill_save" => skill_save(ctx, args),
        "learn_skill" => learn_skill(ctx, args, sink).await,
        name if web_tools::is_web_tool(name) => web_tools::dispatch(name, args).await,
        name if name.starts_with("project_")
            || name.starts_with("terraform_")
            || name.starts_with("cloud_")
            || name.starts_with("tfc_") =>
        {
            infra_tools::dispatch(ctx, call.name.as_str(), args, sink).await
        }
        other => format!("error: unknown tool '{other}'"),
        }
    };

    // PostToolUse hooks: a user-configured command sees the tool result and can feed
    // a note back to the model (a `decision:block` reason) or inject extra context.
    if ctx.hooks.has_event(hooks::HookEvent::PostToolUse) {
        let cwd = hooks::cwd();
        let input = hooks::HookEventInput {
            event: hooks::HookEvent::PostToolUse,
            session_id: &ctx.session_id,
            cwd: &cwd,
            workspace_id: ctx.workspace_id.as_deref(),
            vps_targets: &ctx.targets,
            tool_name: Some(&call.name),
            tool_input: Some(&call.arguments),
            tool_response: Some(&result),
            prompt: None,
        };
        let decision = hooks::run_event(&ctx.hooks, &input).await;
        if let Some(msg) = &decision.system_message {
            emit(Some(sink), StreamEvent::Status(msg.clone()));
        }
        if decision.blocks() {
            let reason = decision
                .reason
                .clone()
                .unwrap_or_else(|| "a PostToolUse hook flagged this result".to_string());
            hook_notes.push(format!("[PostToolUse hook] {reason}"));
        }
        if let Some(extra) = decision.additional_context {
            hook_notes.push(format!("[PostToolUse hook] {extra}"));
        }
    }

    // Append any hook-injected context/feedback so the model sees it alongside the
    // tool result. Kept after the result so it never changes the success/error prefix
    // the loop keys off — except a PostToolUse block, which we surface as a note.
    let result = if hook_notes.is_empty() {
        result
    } else {
        format!("{result}\n\n{}", hook_notes.join("\n"))
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
        "local_run_command" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Run on this PC: {cmd}")
        }
        "local_read_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Read {path} on this PC")
        }
        "local_write_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Write {path} on this PC")
        }
        "local_list_dir" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("List {path} on this PC")
        }
        "upload_file" => {
            let lp = args.get("local_path").and_then(|v| v.as_str()).unwrap_or("…");
            let rp = args.get("remote_path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Upload {lp} → {rp} on {}", vps_label(ctx, args))
        }
        "download_file" => {
            let rp = args.get("remote_path").and_then(|v| v.as_str()).unwrap_or("…");
            let lp = args.get("local_path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Download {rp} from {} → {lp}", vps_label(ctx, args))
        }
        "ssh_setup_key_auth" => format!("Set up SSH key auth on {}", vps_label(ctx, args)),
        "ssh_key_status" => format!("SSH key status for {}", vps_label(ctx, args)),
        "ask_user" => "Ask the user".into(),
        "present_plan" => {
            let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("plan");
            format!("Present plan: {title}")
        }
        "memory_save" => "Save to memory".into(),
        "set_project_brief" => "Update project brief".into(),
        "skill_install" => {
            let src = args.get("source").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Install skill from {src}")
        }
        "list_official_skills" => "List official skills".into(),
        "terminal_send" => {
            let t = args.get("text").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Type in live terminal: {t}")
        }
        "terminal_capture" => "Read live terminal".into(),
        "canvas_open_terminal" => format!("Open terminal on canvas: {}", vps_label(ctx, args)),
        "canvas_open_sftp" => format!("Open SFTP on canvas: {}", vps_label(ctx, args)),
        "canvas_tile" => "Tile the canvas".into(),
        "canvas_close" => "Close canvas panel".into(),
        "canvas_refresh" => "Reconnect terminal".into(),
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
        "learn_skill" => {
            let topic = args.get("topic").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Learn skill · {topic}")
        }
        "web_search" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Web search · {q}")
        }
        "web_fetch" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Web fetch · {url}")
        }
        "geo_locate" => "Locate (by IP)".into(),
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
        "local_run_command" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            emit(
                Some(sink),
                StreamEvent::Activity(ActivityEvent::Command {
                    id: call.id.clone(),
                    vps: "This PC".into(),
                    command,
                }),
            );
        }
        "run_command_all" => {}
        _ => {}
    }
}

/// Whether a tool call would change the local PC or a server (vs. read-only
/// inspection or an interactive prompt). Used by the plan-mode guard. Command
/// tools are mutating only when their command isn't read-only; infra tools are
/// mutating unless they are a plan/validate/show/list-style verb.
pub fn tool_is_mutating(name: &str, args: &Value) -> bool {
    match name {
        // Read-only inspection, agent-local notes/skills, interactive prompts, and
        // non-destructive canvas/UI actions.
        "read_file" | "local_read_file" | "local_list_dir" | "list_vps_targets"
        | "ssh_key_status" | "memory_save" | "skills_list" | "skill_view" | "skill_save"
        | "learn_skill" | "ask_user" | "present_plan" | "terminal_capture" | "canvas_open_terminal"
        | "canvas_open_sftp" | "canvas_tile" | "canvas_close" | "canvas_refresh" => false,
        // Typing into a live shell runs commands → mutating.
        "terminal_send" => true,
        // Shell tools: mutating only when the command isn't read-only.
        "run_command" | "run_command_all" | "local_run_command" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            !safety::is_read_only(cmd)
        }
        // Always change a server or the local PC.
        "write_file" | "local_write_file" | "upload_file" | "download_file"
        | "ssh_setup_key_auth" => true,
        // Web tools (search/fetch/geo) are read-only.
        n if web_tools::is_web_tool(n) => false,
        // Infra tools: allow read-only verbs, treat the rest (apply/destroy/import) as mutating.
        n if n.starts_with("terraform_")
            || n.starts_with("cloud_")
            || n.starts_with("tfc_")
            || n.starts_with("project_") =>
        {
            !(n.contains("plan")
                || n.contains("validate")
                || n.contains("show")
                || n.contains("list")
                || n.contains("get")
                || n.contains("read")
                || n.contains("output")
                || n.contains("fmt")
                || n.contains("version"))
        }
        // Unknown tools: be conservative and treat as mutating.
        _ => true,
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

    #[test]
    fn mutating_classification() {
        // Read-only inspection and interactive prompts are not mutating.
        assert!(!tool_is_mutating("read_file", &json!({})));
        assert!(!tool_is_mutating("local_list_dir", &json!({})));
        assert!(!tool_is_mutating("ssh_key_status", &json!({})));
        assert!(!tool_is_mutating("ask_user", &json!({})));
        assert!(!tool_is_mutating("present_plan", &json!({})));
        // Shell tools depend on whether the command is read-only.
        assert!(!tool_is_mutating("run_command", &json!({"command": "ls -la"})));
        assert!(tool_is_mutating("run_command", &json!({"command": "rm -rf /tmp/x"})));
        assert!(!tool_is_mutating("local_run_command", &json!({"command": "cat /etc/hosts"})));
        // Always-mutating tools.
        assert!(tool_is_mutating("write_file", &json!({})));
        assert!(tool_is_mutating("local_write_file", &json!({})));
        assert!(tool_is_mutating("upload_file", &json!({})));
        assert!(tool_is_mutating("ssh_setup_key_auth", &json!({})));
        // Infra: plan is read-only, apply mutates.
        assert!(!tool_is_mutating("terraform_plan", &json!({})));
        assert!(tool_is_mutating("terraform_apply", &json!({})));
    }
}

/// Run a command on a specific VPS without emitting a `$ command` status line
/// (the caller renders its own UI).
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
    let base = safety::effective_mode(&ctx.db, &ctx.safety, vps_id);
    let mode = safety::resolve_session_mode(&ctx.session_state, &ctx.session_id, &base);
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

async fn run_command(ctx: &ToolContext, args: &Value, _sink: &EventSink, _id: &str) -> String {
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
    // The ToolResult is emitted once by the agent loop (run_command emits a
    // byte-identical one otherwise). Per-target/snapshot emits use distinct ids.
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
    // Capture the file's current content first so the changes panel can diff it.
    let before = ctx
        .sessions
        .run_command(&vps_id, &format!("cat -- {}", shell_quote(&path)))
        .await
        .map(|o| o.stdout)
        .unwrap_or_default();
    // Transfer via base64 to avoid any quoting/encoding issues.
    let b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        shell_quote(parent),
        shell_quote(&b64),
        shell_quote(&path)
    );
    let result = exec(ctx, &vps_id, &command, Some(sink), true).await;
    if result.starts_with("exit_code: 0") {
        let label = ctx
            .db
            .get_vps(&vps_id)
            .ok()
            .flatten()
            .map(|v| format!("{} ({})", v.name, v.host))
            .unwrap_or_else(|| vps_id.clone());
        ctx.edits.record(
            &ctx.app,
            &ctx.session_id,
            "vps",
            Some(vps_id.clone()),
            &label,
            &path,
            &before,
            content,
        );
    }
    result
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
    // Workspace-scoped memory when a workspace is active; else global memory.
    if let Some(ws) = ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) {
        return match workspace_context::append_memory(&ctx.home, ws, entry) {
            Ok(()) => "saved to this workspace's memory".into(),
            Err(e) => format!("error: {e}"),
        };
    }
    match memory::append_memory(&ctx.home, entry) {
        Ok(_) => "saved to memory".into(),
        Err(e) => format!("error: {e}"),
    }
}

fn set_project_brief(ctx: &ToolContext, args: &Value) -> String {
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let ws = match ctx.workspace_id.as_deref().filter(|s| !s.is_empty()) {
        Some(w) => w,
        None => {
            return "error: no active workspace — a project brief is per-workspace. Ask the user to \
                    select a workspace first."
                .into()
        }
    };
    match workspace_context::save_brief(&ctx.home, ws, content) {
        Ok(()) => "saved the project brief for this workspace".into(),
        Err(e) => format!("error: {e}"),
    }
}

/// Download → scan → gate → install a skill. Failing scans are blocked; untrusted
/// sources require approval; the official Anthropic repo installs without prompting.
async fn skill_install_tool(ctx: &ToolContext, args: &Value) -> String {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return "error: missing 'source' (a URL to a SKILL.md or a skill folder)".into(),
    };

    let skill_md = match skill_install::fetch_skill_md(&source).await {
        Ok(t) => t,
        Err(e) => return e,
    };

    // Stage to a temp dir so the scanner sees the file on disk.
    let tmp = std::env::temp_dir().join(format!("xconsole_skill_{}", Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&tmp) {
        return format!("error: staging skill: {e}");
    }
    if let Err(e) = std::fs::write(tmp.join("SKILL.md"), &skill_md) {
        let _ = std::fs::remove_dir_all(&tmp);
        return format!("error: staging skill: {e}");
    }
    let report = skill_scan::scan_skill(&tmp).await;
    let _ = std::fs::remove_dir_all(&tmp);

    if report.is_blocking() {
        return format!(
            "BLOCKED: this skill failed the security scan and was NOT installed.\n{}",
            report.summary()
        );
    }

    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("downloaded")
        .to_string();
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| skill_install::derive_name(&source));

    // Untrusted source → require approval (the official Anthropic repo is trusted).
    if !skill_scan::is_trusted_source(&source) {
        let gate = format!(
            "install skill '{category}/{name}' from {source} \
             (scan: {} risk {}/100, scanner {})",
            report.severity, report.risk_score, report.scanner
        );
        if let Err(e) = authorize_local(ctx, &gate).await {
            return format!("error: {e}");
        }
    }

    match skills::save_skill(&ctx.home, &category, &name, &skill_md) {
        Ok(()) => format!("Installed skill {category}/{name}.\n{}", report.summary()),
        Err(e) => format!("error: {e}"),
    }
}

// ---- Agent control of the live canvas -------------------------------------

async fn terminal_send(ctx: &ToolContext, args: &Value) -> String {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) if !t.is_empty() => t,
        _ => return "error: missing 'text'".into(),
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let sessions = ctx.sessions.live_sessions_for_vps(&vps_id);
    if sessions.is_empty() {
        return "error: no terminal is open on the canvas for that server — open one with \
                canvas_open_terminal first, or use run_command for a private command."
            .into();
    }
    // Typing into a live shell runs commands → gate like any command.
    if let Err(e) = authorize_vps(ctx, &vps_id, &format!("type into live terminal: {text}")).await {
        return format!("error: {e}");
    }
    let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut payload = text.to_string();
    if submit && !payload.ends_with('\r') && !payload.ends_with('\n') {
        payload.push('\r');
    }
    // Send to the first live terminal for this server (avoid double-running).
    match ctx.sessions.write(&sessions[0], payload.as_bytes()) {
        Ok(()) => "sent input to the live terminal. Use terminal_capture to read the result.".into(),
        Err(e) => format!("error: {e}"),
    }
}

fn terminal_capture(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let sessions = ctx.sessions.live_sessions_for_vps(&vps_id);
    let Some(sid) = sessions.first() else {
        return "error: no terminal is open on the canvas for that server.".into();
    };
    let text = ctx.sessions.capture_text(sid).unwrap_or_default();
    let trimmed = text.trim_end();
    // Return the tail (recent screen) to keep it compact.
    if trimmed.len() > 4000 {
        let start = trimmed.len() - 4000;
        let cut = (start..trimmed.len())
            .find(|&i| trimmed.is_char_boundary(i))
            .unwrap_or(start);
        format!("…(earlier output trimmed)\n{}", &trimmed[cut..])
    } else if trimmed.is_empty() {
        "(terminal is empty)".into()
    } else {
        trimmed.to_string()
    }
}

/// Emit a canvas action to the frontend (open/close a node). Resolves the VPS the
/// same way other tools do so it stays within the selected targets.
fn canvas_command_tool(ctx: &ToolContext, args: &Value, action: &str) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let _ = ctx.app.emit(
        "canvas://command",
        json!({ "action": action, "vps_id": vps_id }),
    );
    let label = ctx
        .db
        .get_vps(&vps_id)
        .ok()
        .flatten()
        .map(|v| v.name)
        .unwrap_or(vps_id);
    format!("requested canvas action '{action}' for {label}")
}

/// Close / reconnect a canvas panel. Prefers an explicit `node_id` (one specific
/// panel from the Live canvas list); otherwise falls back to `vps_id` (all panels
/// for that server). Used by canvas_close and canvas_refresh.
fn canvas_node_command(ctx: &ToolContext, args: &Value, action: &str) -> String {
    if let Some(node_id) = args.get("node_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let _ = ctx
            .app
            .emit("canvas://command", json!({ "action": action, "node_id": node_id }));
        return format!("requested '{action}' for that panel");
    }
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let _ = ctx
        .app
        .emit("canvas://command", json!({ "action": action, "vps_id": vps_id }));
    let label = ctx
        .db
        .get_vps(&vps_id)
        .ok()
        .flatten()
        .map(|v| v.name)
        .unwrap_or(vps_id);
    format!("requested '{action}' for {label}")
}

fn canvas_tile_tool(ctx: &ToolContext) -> String {
    let _ = ctx
        .app
        .emit("canvas://command", json!({ "action": "tile" }));
    "tiled the canvas".into()
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

/// Autoresearch: research an unfamiliar capability on the web and build a quarantined,
/// security-scanned skill the agent can apply immediately. Resolves the active provider
/// for the (low-temperature) synthesis call. See `ai::autoresearch`.
async fn learn_skill(ctx: &ToolContext, args: &Value, sink: &EventSink) -> String {
    let topic = match args.get("topic").and_then(|v| v.as_str()) {
        Some(t) if !t.trim().is_empty() => t.trim(),
        _ => return "error: missing 'topic'".into(),
    };
    let name_hint = args.get("name").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty());

    // Resolve a provider for synthesis (the active agent provider).
    let provider_id = match crate::ai::registry::active_provider_id(&ctx.db, None) {
        Ok(id) => id,
        Err(e) => return format!("error: cannot research — no AI provider available ({e})"),
    };
    let resolved = match crate::ai::registry::build(&ctx.db, &provider_id) {
        Ok(r) => r,
        Err(e) => return format!("error: cannot research — provider unavailable ({e})"),
    };

    // The user's own server hostnames/IPs, scrubbed from the outbound search query.
    let mut known_hosts: Vec<String> = Vec::new();
    for id in &ctx.targets {
        if let Ok(Some(vps)) = ctx.db.get_vps(id) {
            known_hosts.push(vps.host);
            known_hosts.push(vps.name);
        }
    }

    let result = crate::ai::autoresearch::learn(
        &ctx.home,
        resolved.provider.as_ref(),
        &resolved.model,
        topic,
        name_hint,
        &known_hosts,
        None,
        Some(sink),
    )
    .await;

    // Surface a saved skill in the activity feed like skill_save does.
    if result.status == crate::ai::autoresearch::LearnStatus::Saved {
        emit(
            Some(sink),
            StreamEvent::Activity(ActivityEvent::SkillSaved {
                id: String::new(),
                category: result.category.clone(),
                name: result.name.clone(),
            }),
        );
    }
    result.to_tool_result()
}

// ---- Local-PC tools (this machine, not a VPS) -----------------------------

/// Format a local command's output identically to the VPS path (`exec_inner`).
fn format_local_output(out: &crate::ssh::manager::CommandOutput) -> String {
    let mut s = format!("exit_code: {}\n", out.exit_code);
    if !out.stdout.is_empty() {
        s.push_str(&format!("stdout:\n{}\n", out.stdout.trim_end()));
    }
    if !out.stderr.is_empty() {
        s.push_str(&format!("stderr:\n{}\n", out.stderr.trim_end()));
    }
    s
}

/// Gate a local action through the session safety mode (no VPS target).
async fn authorize_local(ctx: &ToolContext, gate_command: &str) -> Result<(), String> {
    let mode = safety::resolve_session_mode(&ctx.session_state, &ctx.session_id, &ctx.safety);
    safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &mode,
        &ctx.session_id,
        None,
        gate_command,
    )
    .await
}

/// Gate a VPS-targeted action through that VPS's effective safety mode.
async fn authorize_vps(ctx: &ToolContext, vps_id: &str, gate_command: &str) -> Result<(), String> {
    let base = safety::effective_mode(&ctx.db, &ctx.safety, vps_id);
    let mode = safety::resolve_session_mode(&ctx.session_state, &ctx.session_id, &base);
    safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &mode,
        &ctx.session_id,
        Some(vps_id),
        gate_command,
    )
    .await
}

async fn local_run_command(ctx: &ToolContext, args: &Value) -> String {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return "error: missing 'command'".into(),
    };
    if let Err(e) = authorize_local(ctx, command).await {
        return format!("error: {e}");
    }
    match crate::local::run_local_command(command).await {
        Ok(out) => format_local_output(&out),
        Err(e) => format!("error running command: {e}"),
    }
}

async fn local_read_file(ctx: &ToolContext, args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };
    // Gate as a read (allowlisted, so it auto-runs under allowlist mode).
    if let Err(e) = authorize_local(ctx, &format!("cat -- {}", shell_quote(path))).await {
        return format!("error: {e}");
    }
    match crate::local::read_local_file(path) {
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    }
}

async fn local_write_file(ctx: &ToolContext, args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if let Err(e) = authorize_local(ctx, &format!("write local file {path}")).await {
        return format!("error: {e}");
    }
    let before = crate::local::read_local_file(path).ok();
    match crate::local::write_local_file(path, content) {
        Ok(()) => {
            ctx.edits.record(
                &ctx.app,
                &ctx.session_id,
                "local",
                None,
                "This PC",
                path,
                before.as_deref().unwrap_or(""),
                content,
            );
            format!("wrote {} bytes to {path}", content.len())
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn local_list_dir(ctx: &ToolContext, args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };
    if let Err(e) = authorize_local(ctx, &format!("ls -- {}", shell_quote(path))).await {
        return format!("error: {e}");
    }
    match crate::local::list_local_dir(path) {
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    }
}

// ---- Local <-> VPS file transfer ------------------------------------------

/// Cap transfers at the same size the SFTP path uses (10 MB).
const MAX_TRANSFER: usize = 10 * 1024 * 1024;

async fn upload_file(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let local_path = match args.get("local_path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'local_path'".into(),
    };
    let remote_path = match args.get("remote_path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'remote_path'".into(),
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let bytes = match std::fs::read(local_path) {
        Ok(b) => b,
        Err(e) => return format!("error: reading local file: {e}"),
    };
    if bytes.len() > MAX_TRANSFER {
        return format!(
            "error: file too large ({} bytes, max {MAX_TRANSFER})",
            bytes.len()
        );
    }
    let gate = format!(
        "upload {local_path} (local) -> {remote_path} ({} bytes)",
        bytes.len()
    );
    if let Err(e) = authorize_vps(ctx, &vps_id, &gate).await {
        return format!("error: {e}");
    }

    let parent = std::path::Path::new(remote_path)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|p| !p.is_empty())
        .unwrap_or("/tmp");
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        shell_quote(parent),
        shell_quote(&b64),
        shell_quote(remote_path)
    );

    let activity_id = format!("upload-{vps_id}");
    emit_command_activity(ctx, sink, &activity_id, &vps_id, &format!("upload → {remote_path}"));
    let result = match ctx.sessions.run_command(&vps_id, &command).await {
        Ok(out) if out.exit_code == 0 => format!("uploaded {} bytes to {remote_path}", bytes.len()),
        Ok(out) => format!("error: upload failed: {}", out.stderr.trim()),
        Err(e) => format!("error: {e}"),
    };
    emit_command_result(sink, &activity_id, &result);
    result
}

async fn download_file(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let remote_path = match args.get("remote_path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'remote_path'".into(),
    };
    let local_path = match args.get("local_path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'local_path'".into(),
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let gate = format!("download {remote_path} (server) -> {local_path} (local)");
    if let Err(e) = authorize_vps(ctx, &vps_id, &gate).await {
        return format!("error: {e}");
    }

    let activity_id = format!("download-{vps_id}");
    emit_command_activity(ctx, sink, &activity_id, &vps_id, &format!("download {remote_path}"));
    let read_cmd = format!("base64 -- {}", shell_quote(remote_path));
    let out = match ctx.sessions.run_command(&vps_id, &read_cmd).await {
        Ok(o) => o,
        Err(e) => {
            let m = format!("error: {e}");
            emit_command_result(sink, &activity_id, &m);
            return m;
        }
    };
    if out.exit_code != 0 {
        let m = format!("error: reading remote file: {}", out.stderr.trim());
        emit_command_result(sink, &activity_id, &m);
        return m;
    }
    // base64 output may be wrapped across lines — strip all whitespace before decoding.
    let b64: String = out.stdout.split_whitespace().collect();
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) {
        Ok(b) => b,
        Err(e) => {
            let m = format!("error: decoding remote file: {e}");
            emit_command_result(sink, &activity_id, &m);
            return m;
        }
    };
    if bytes.len() > MAX_TRANSFER {
        let m = format!("error: file too large ({} bytes, max {MAX_TRANSFER})", bytes.len());
        emit_command_result(sink, &activity_id, &m);
        return m;
    }
    if let Some(parent) = std::path::Path::new(local_path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let m = format!("error: creating local directory: {e}");
                emit_command_result(sink, &activity_id, &m);
                return m;
            }
        }
    }
    let result = match std::fs::write(local_path, &bytes) {
        Ok(()) => format!("downloaded {} bytes to {local_path}", bytes.len()),
        Err(e) => format!("error: writing local file: {e}"),
    };
    emit_command_result(sink, &activity_id, &result);
    result
}

// ---- SSH key lifecycle ----------------------------------------------------

async fn ssh_setup_key_auth(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let gate =
        "set up SSH key authentication: generate a keypair, install the public key on the server, \
         and switch it to key login (password login stays enabled)";
    if let Err(e) = authorize_vps(ctx, &vps_id, gate).await {
        return format!("error: {e}");
    }
    match keygen::setup_key_auth(&ctx.db, &ctx.sessions, &vps_id).await {
        Ok(r) => format!(
            "Key authentication is set up.\nFingerprint: {}\nInstalled public key: {}\n\
             The private key is stored only in your OS keychain (never on disk or in the database). \
             Password login on the server was left enabled as a fallback.",
            r.fingerprint, r.public_openssh
        ),
        Err(e) => format!("error: {e}"),
    }
}

fn ssh_key_status(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let vps = match ctx.db.get_vps(&vps_id) {
        Ok(Some(v)) => v,
        Ok(None) => return "error: VPS not found".into(),
        Err(e) => return format!("error: {e}"),
    };
    let managed = secrets::has_secret(&secrets::ssh_key_key(&vps_id));
    let key_path = vps.key_path.clone().unwrap_or_else(|| "(none)".into());
    format!(
        "auth_type: {}\nmanaged_key_in_keychain: {}\nkey_path: {}",
        vps.auth_type.as_str(),
        managed,
        key_path
    )
}

// ---- Interactive prompts: clarifying questions and plan review --------------

async fn ask_user(ctx: &ToolContext, args: &Value) -> String {
    let questions = args.get("questions").filter(|q| {
        q.as_array().map(|a| !a.is_empty()).unwrap_or(false)
    });
    let questions = match questions {
        Some(q) => q.clone(),
        None => return "error: missing 'questions' (a non-empty array)".into(),
    };
    let id = Uuid::new_v4().to_string();
    let payload = json!({
        "id": id,
        "session_id": ctx.session_id,
        "questions": questions,
    });
    let _ = ctx.app.emit("ai://question", payload);
    let rx = ctx.prompts.register(id.clone());
    match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
        Ok(Ok(answer)) if !answer.trim().is_empty() => format!("User's answer:\n{}", answer.trim()),
        Ok(Ok(_)) => "The user submitted an empty answer.".into(),
        Ok(Err(_)) => "error: question channel closed".into(),
        Err(_) => {
            ctx.prompts.cancel(&id);
            "error: the user did not answer in time".into()
        }
    }
}

async fn present_plan(ctx: &ToolContext, args: &Value) -> String {
    let plan = match args.get("plan").and_then(|v| v.as_str()) {
        Some(p) if !p.trim().is_empty() => p,
        _ => return "error: missing 'plan'".into(),
    };
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Plan");
    let id = Uuid::new_v4().to_string();
    let payload = json!({
        "id": id,
        "session_id": ctx.session_id,
        "title": title,
        "plan": plan,
    });
    let _ = ctx.app.emit("ai://plan", payload);
    let rx = ctx.prompts.register(id.clone());
    match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
        Ok(Ok(decision)) => {
            // The frontend sends "APPROVE" or "REJECT: <feedback>".
            let d = decision.trim();
            if d.eq_ignore_ascii_case("approve") || d.to_ascii_uppercase().starts_with("APPROVE") {
                ctx.session_state.mark_plan_approved(&ctx.session_id);
                "The user APPROVED the plan. Proceed to execute it now.".into()
            } else {
                let feedback = d
                    .strip_prefix("REJECT:")
                    .or_else(|| d.strip_prefix("reject:"))
                    .map(|s| s.trim())
                    .unwrap_or(d);
                if feedback.is_empty() {
                    "The user rejected the plan. Ask what they want changed, or revise and call \
                     present_plan again."
                        .into()
                } else {
                    format!(
                        "The user requested changes to the plan: {feedback}\nRevise the plan and \
                         call present_plan again."
                    )
                }
            }
        }
        Ok(Err(_)) => "error: plan channel closed".into(),
        Err(_) => {
            ctx.prompts.cancel(&id);
            "error: the user did not respond to the plan in time".into()
        }
    }
}
