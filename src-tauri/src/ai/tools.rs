//! Agent tools and dispatch. One dispatch function maps a tool call to an action
//! (SSH command, file read/write, memory, skills) and applies the safety gate.

use base64::Engine;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::ai::infra_tools;
use crate::ai::jobs;
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
    /// Lifecycle hooks (snapshotted at startup). Empty = disabled.
    pub hooks: crate::ai::hooks::HooksConfig,
    /// Images from the latest user turn (for the `vision` side-call). Empty when
    /// the session model already received native image blocks.
    pub turn_images: Vec<crate::ai::provider::ChatImage>,
    /// Intake goal id for a normal chat turn (`/goal`). Loop cycles use `goal:<id>`
    /// as the session id instead.
    pub goal_id: Option<String>,
    /// Which named agent this turn *is*.
    ///
    /// A goal carries its persona on the goal row, but a turn that is not a goal had no
    /// way to be anybody — so a message arriving over remote chat always ran as the
    /// unnamed main agent, which then had to relay to the lead agent. The user asked a
    /// question on WhatsApp and got an answer from a middleman.
    pub persona_id: Option<String>,
    /// This turn may look but not touch.
    ///
    /// Distinct from `plan_mode`, which also tells the agent to draft a plan and wait
    /// for approval. An exploration sub-agent is not drafting anything — it is being
    /// asked a question, and the answer is the whole job.
    pub read_only: bool,
}

/// Tool schemas advertised to the model.
pub fn definitions(_home: &AgentHome) -> Vec<ToolDef> {
    let mut defs = vec![
        ToolDef {
            name: "run_command".into(),
            description: "Run a shell command on one server over SSH. When multiple targets are \
selected, vps_id is required (exact UUID from the target list). Commands run in the \
foreground time out after 120s — for anything slower (builds, apt/dnf upgrades, docker \
pulls, rsync, migrations) pass background:true instead of splitting the work up or asking \
the user to run it. That returns a job_id immediately and the command keeps running on the \
server; check it later with job_status.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to run."},
                    "vps_id": {"type": "string", "description": "Exact target UUID. Required when more than one VPS is selected."},
                    "timeout_secs": {"type": "integer", "description": "How long to wait, in seconds (default 120, max 3600). Raise it for a build or a migration you want to watch to the end; past a few minutes prefer background:true, which survives the turn."},
                    "background": {
                        "type": "boolean",
                        "description": "Run detached and return a job_id straight away. Use for anything that may take over ~2 minutes. The job survives this turn, the session, and an xConsole restart."
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "job_status".into(),
            description: "Check a background job started with run_command(background:true) or \
local_run_command(background:true): whether it is still running, its exit code once finished, \
and the tail of its output. Poll this instead of waiting — do other useful work between \
checks.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "local": {"type": "boolean", "description": "The job is on this PC rather than a server. Set it when the job was started by local_run_command."},
                    "job_id": {"type": "string", "description": "The job_id returned when the job was started."},
                    "tail_lines": {"type": "integer", "description": "Lines of output to return (default 40, max 500)."},
                    "vps_id": {"type": "string", "description": "The server the job runs on. Required when more than one VPS is selected."}
                },
                "required": ["job_id"]
            }),
        },
        ToolDef {
            name: "job_list".into(),
            description: "List background jobs with their state and command. Use it to pick up \
jobs started in an earlier session — a job lives on the machine that runs it, not in this chat, \
so it survives the session and an xConsole restart."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "local": {"type": "boolean", "description": "The job is on this PC rather than a server. Set it when the job was started by local_run_command."},
                    "vps_id": {"type": "string", "description": "Exact target UUID. Required when more than one VPS is selected."}
                }
            }),
        },
        ToolDef {
            name: "job_kill".into(),
            description: "Stop a background job (SIGTERM, then SIGKILL if it ignores that). \
Signals the whole process group, so work the job spawned stops too.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "local": {"type": "boolean", "description": "The job is on this PC rather than a server. Set it when the job was started by local_run_command."},
                    "job_id": {"type": "string"},
                    "vps_id": {"type": "string", "description": "Exact target UUID. Required when more than one VPS is selected."}
                },
                "required": ["job_id"]
            }),
        },
        ToolDef {
            name: "run_command_all".into(),
            description: "Run the same shell command on every selected VPS target and return \
combined output. Prefer this when the user asks about both/all/each server. Targets run one \
after another, so the time is the sum: for anything slow, start it per server with \
run_command(background:true) instead and collect the job_ids.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to run on each target."},
                    "timeout_secs": {"type": "integer", "description": "How long to wait per target, in seconds (default 120, max 3600)."}
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
            description: "Read text files from a server. Lines are numbered. Pass `paths` to open \
several at once — a config, the file it includes and the unit that starts it is one call, not \
three round trips. For one large file, pass offset (1-based line) and limit instead of reading \
everything; find the line with grep_search first. A negative offset reads from the end, so \
offset:-200 is the tail of a log.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "One file. Use `paths` for several."},
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Several files in one round trip (max 20). offset/limit do not apply here — read the batch to see what is there, then re-read one file with a window."
                    },
                    "offset": {"type": "integer", "description": "1-based start line. Negative counts back from the end, so offset:-100 reads the last 100 lines of a log — cheaper and safer than shelling out to `tail`, and it keeps the file's freshness stamp. Single file only."},
                    "limit": {"type": "integer", "description": "Max lines to return. Single file only."},
                    "vps_id": {"type": "string"}
                }
            }),
        },
        ToolDef {
            name: "write_file".into(),
            description: "Write (overwrite) a text file on a server. Subject to the safety mode. \
Use /root/ or /tmp/ on Linux (root login) — not /home/root/. Prefer hello.py over names with spaces. \
For an existing file prefer edit_file (replace a unique snippet) — cheaper and safer. \
Content too long for one call: write the first part, then call again with mode:append."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "mode": {"type": "string", "enum": ["overwrite", "append"], "description": "overwrite (default) replaces the file; append adds to the end. For a file too long to send in one call, write the first part then append the rest."},
                    "vps_id": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "edit_file".into(),
            description: "Replace snippets in an existing file on a server. Prefer this over \
write_file for any file you have already read. old_string must match exactly once unless \
replace_all is true. To change several places, pass `edits` and do it in one call — a separate \
call per edit reads and rewrites the whole file each time.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string", "description": "For a single edit. Ignored when `edits` is given."},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean"},
                    "edits": {
                        "type": "array",
                        "description": "Several replacements in one call, applied in order. Use this whenever you are changing more than one place in a file: each single edit reads and rewrites the whole file, so five separate calls is five of each. If any one fails, none are applied.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": {"type": "string"},
                                "new_string": {"type": "string"},
                                "replace_all": {"type": "boolean"}
                            },
                            "required": ["old_string", "new_string"]
                        }
                    },
                    "vps_id": {"type": "string"}
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "grep_search".into(),
            description: "Search file *contents* on a server with a regex. Use this FIRST on large \
trees instead of reading whole files. Pass context_lines to see what a match is doing without a \
second read_file call, and output_mode=files when the question is only where something lives. To \
find files by *name* instead, use find_files.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "path": {"type": "string", "description": "File or directory to search (default /)."},
                    "glob": {"type": "string", "description": "Optional filename glob, e.g. *.conf"},
                    "case_insensitive": {"type": "boolean"},
                    "context_lines": {"type": "integer", "description": "Lines of context around each match (like grep -C). Use 2-5 to read what a match is doing without a second call to read_file — usually that is the whole reason for the follow-up read."},
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files", "count"],
                        "description": "content = matching lines (default); files = just which files contain it, for 'where does this live'; count = how many per file, for 'how widespread is this'."
                    },
                    "multiline": {"type": "boolean", "description": "Let the pattern span lines (needs ripgrep on the host)."},
                    "head_limit": {"type": "integer", "description": "Max output lines (default 250, max 2000)."},
                    "offset": {"type": "integer", "description": "Skip this many matching lines first. Use the offset the previous call reported to read on instead of narrowing a search you cannot see the end of."},
                    "vps_id": {"type": "string"}
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "list_dir".into(),
            description: "List a directory on a server: names, sizes, modification times and \
what is a directory. This is the orientation call — use it when you do not yet know what is in a \
place, before grepping or reading. To find a file whose location you do not know, use find_files \
instead."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory to list."},
                    "all": {"type": "boolean", "description": "Include dotfiles. Default true, because a hidden file is usually exactly what is being looked for."},
                    "head_limit": {"type": "integer", "description": "Max entries (default 100)."},
                    "vps_id": {"type": "string"}
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "find_files".into(),
            description: "Find files on a server by name pattern, newest first. Use this when you \
know roughly what a file is called but not where it lives (nginx configs, *.service units, a \
compose file, logs) — do not grep the whole disk for a filename.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Filename glob, e.g. *.conf, docker-compose.y*ml, *.service"},
                    "path": {"type": "string", "description": "Directory to search under (default /)."},
                    "type": {"type": "string", "enum": ["file", "dir", "any"], "description": "What to match (default file)."},
                    "head_limit": {"type": "integer", "description": "Max results (default 200, max 2000)."},
                    "vps_id": {"type": "string"}
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "todo_write".into(),
            description: "Replace your live working checklist for THIS chat. Use for any task with \
3+ steps AFTER the user is ready for you to execute (not instead of present_plan). Each item: \
content, activeForm (gerund shown while in progress), status pending|in_progress|completed. \
Keep exactly one in_progress. Mark done as you finish so you do not repeat work. The current \
list is shown to you every turn under # Todos.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {"type": "string"},
                                "activeForm": {"type": "string"},
                                "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                            },
                            "required": ["content", "status"]
                        }
                    }
                },
                "required": ["todos"]
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
            name: "rename_session".into(),
            description: "Rename this agent conversation session to a clear, descriptive title summarizing the topic or work (e.g. when the user asks to rename the session or when establishing the main task).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "The new short, descriptive title for this session."
                    }
                },
                "required": ["title"]
            }),
        },
        // ---- Goal-driven autonomous mode (/goal) ----
        ToolDef {
            name: "goal_propose_spec".into(),
            description: "Intake phase for a goal session: submit the drafted GoalSpec (objective, \
success criteria, check method/tooling, hard constraints, max cycles) for the user to review and \
lock. Only callable while the goal is in 'intake' status.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "objective": {"type": "string"},
                    "success_criteria": {"type": "array", "items": {"type": "string"}},
                    "check_method": {"type": "string"},
                    "check_tooling": {"type": "array", "items": {"type": "string"}},
                    "hard_constraints": {"type": "array", "items": {"type": "string"}},
                    "max_cycles": {"type": "integer"}
                },
                "required": ["objective", "success_criteria", "check_method"]
            }),
        },
        ToolDef {
            name: "goal_add_task".into(),
            description: "Add a kanban card to the goal board. Columns: backlog, in_progress, \
waiting, testing, blocked, done. kind: edit, test, bug, research, check. Pass parent_id to \
create a sub-task under an existing card — break work into sub-tasks whenever a step has \
more than one action.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "column": {"type": "string"},
                    "title": {"type": "string"},
                    "kind": {"type": "string"},
                    "detail": {"type": "string"},
                    "files": {"type": "array", "items": {"type": "string"}},
                    "parent_id": {"type": "string", "description": "Existing task id to nest this under"}
                },
                "required": ["column", "title"]
            }),
        },
        ToolDef {
            name: "goal_update_task".into(),
            description: "Move or annotate an existing kanban card (column, result, error, files, \
detail, note). Always write a note of what you just did so the task history stays complete. \
The task id is returned by goal_add_task.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "column": {"type": "string"},
                    "title": {"type": "string"},
                    "detail": {"type": "string"},
                    "kind": {"type": "string"},
                    "result": {"type": "string"},
                    "error": {"type": "string"},
                    "note": {"type": "string", "description": "What happened — appended to task history"},
                    "files": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "goal_record_constraint".into(),
            description: "Write a learned fact to the goal's constraint memory (e.g. how long \
Google takes to reindex). Provide the key, value, and the evidence you observed.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {"type": "string"},
                    "value": {"type": "string"},
                    "evidence": {"type": "string"},
                    "confidence": {"type": "string"}
                },
                "required": ["key", "value", "evidence"]
            }),
        },
        ToolDef {
            name: "goal_check_criteria".into(),
            description: "THE verification gate. Verdicts: 'met' (done, with evidence), \
'not_yet' (keep working), 'too_early_to_tell' (keep working unless delay_secs is set). \
Do not wait unless the user asked for a timeout.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "verdict": {"type": "string", "enum": ["met", "not_yet", "too_early_to_tell"]},
                    "evidence": {"type": "string"},
                    "delay_secs": {"type": "integer", "description": "Optional wait before the next cycle. Omit to keep going."}
                },
                "required": ["verdict", "evidence"]
            }),
        },
        ToolDef {
            name: "goal_schedule_wait".into(),
            description: "Explicitly pause the goal until a time (RFC3339) with a reason — e.g. \
waiting for Google to reindex. Writes next_check_at; the loop resumes then.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "until": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["until", "reason"]
            }),
        },
        ToolDef {
            name: "host_memory_get".into(),
            description: "Read the institutional dossier (PROFILE + MEMORY) for one VPS. \
Prefer this over guessing OS/stack/history. Use the exact vps_id from the target list."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string", "description": "Exact target UUID."}
                },
                "required": ["vps_id"]
            }),
        },
        ToolDef {
            name: "host_memory_update".into(),
            description: "Update knowledge about one VPS. Use kind=profile to set/replace PROFILE.md \
(role, OS, stack, services). Use kind=memory to append a durable fact about THIS host only. \
Never store secrets/passwords."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string"},
                    "kind": {"type": "string", "enum": ["profile", "memory"]},
                    "content": {"type": "string", "description": "Full PROFILE body, or one memory bullet."}
                },
                "required": ["vps_id", "kind", "content"]
            }),
        },
        ToolDef {
            name: "taste_save".into(),
            description: "Save a user preference (how they like ops done, or their profile) to the \
merged TASTE.md store. Examples: prefer systemd restarts, never apt upgrade without approval, \
terse replies. Keep terse."
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
                    "command": {"type": "string", "description": "The shell command to run on this PC."},
                    "timeout_secs": {"type": "integer", "description": "How long to wait, in seconds (default 120, max 3600). Raise it for a build or an install rather than splitting work that has to run in one go."},
                    "background": {"type": "boolean", "description": "Run detached and return a job_id straight away. Use for anything over a few minutes — a build, an install, a large copy. The job outlives this turn and an xConsole restart; follow it with job_status(local: true)."}
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "local_read_file".into(),
            description: "Read text files from this PC. Lines are numbered. Pass `paths` to open \
several at once. Use offset/limit on a single large file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "One file. Use `paths` for several."},
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Several files at once (max 20). offset/limit do not apply."
                    },
                    "offset": {"type": "integer", "description": "1-based start line. Negative counts back from the end, so offset:-100 reads the last 100 lines — use it to tail a log instead of shelling out to `tail`. Single file only."},
                    "limit": {"type": "integer", "description": "Max lines. Single file only."}
                }
            }),
        },
        ToolDef {
            name: "local_edit_file".into(),
            description: "Replace snippets in a file on this PC. Prefer this over \
local_write_file for existing files. To change several places, pass `edits` and do it in one \
call.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string", "description": "For a single edit. Ignored when `edits` is given."},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean"},
                    "edits": {
                        "type": "array",
                        "description": "Several replacements in one call, applied in order. If any one fails, none are applied.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": {"type": "string"},
                                "new_string": {"type": "string"},
                                "replace_all": {"type": "boolean"}
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "local_grep_search".into(),
            description: "Search file contents on this PC. Pass context_lines to see what a \
match is doing without a second read, and output_mode=files when the question is only where \
something lives.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string"},
                    "path": {"type": "string"},
                    "glob": {"type": "string"},
                    "case_insensitive": {"type": "boolean"},
                    "context_lines": {"type": "integer", "description": "Lines of context around each match (like grep -C)."},
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files", "count"],
                        "description": "content = matching lines (default); files = which files contain it; count = how many per file."
                    },
                    "multiline": {"type": "boolean", "description": "Let the pattern span lines (needs ripgrep)."},
                    "head_limit": {"type": "integer", "description": "Max output lines (default 250, max 2000)."},
                    "offset": {"type": "integer", "description": "Skip this many matching lines first, to read past a truncated result."}
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "local_write_file".into(),
            description: "Write (overwrite) a text file on the user's local machine (this PC). \
Parent directories are created automatically. Subject to the safety mode. For a file that \
already exists prefer local_edit_file (replace a unique snippet) — cheaper, and it cannot lose \
the parts of the file you did not mean to touch.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "mode": {"type": "string", "enum": ["overwrite", "append"], "description": "overwrite (default) replaces the file; append adds to the end. For a file too long to send in one call, write the first part then append the rest."}
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "local_find_files".into(),
            description: "Find files on this PC by name pattern, newest first. The twin of \
find_files — use it when you know roughly what a file is called but not where it lives, instead \
of grepping the whole disk for a filename."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Filename glob, e.g. *.conf, docker-compose.y*ml"},
                    "path": {"type": "string", "description": "Directory to search under (default the current one)."},
                    "type": {"type": "string", "enum": ["file", "dir", "any"], "description": "What to match (default file)."},
                    "head_limit": {"type": "integer", "description": "Max results (default 200, max 2000)."}
                },
                "required": ["pattern"]
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
private key in the OS keychain AND a verified backup file on this PC, point the xConsole VPS \
record at that key, and verify key login works. Password login on the server is left enabled \
(no lockout). The private key is NEVER returned to you — only the path, fingerprint, and SHA-256. \
Use this before disabling password SSH.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string"},
                    "backup_dir": {
                        "type": "string",
                        "description": "Optional folder on this PC for the key backup. Default: xConsole artifacts/ssh/<server>/"
                    },
                    "save_backup": {
                        "type": "boolean",
                        "description": "Write a verified local backup (default true). Set false only if the user refuses a file backup."
                    }
                },
                "required": []
            }),
        },
        ToolDef {
            name: "vps_update_login".into(),
            description: "Update how xConsole connects to a saved server: host, port, username, \
auth_type (key/password), or key_path. Use this after changing sshd port or switching to a key \
file. You CANNOT read or set the password, and you NEVER receive key material. After changing \
the remote sshd port, call this so xConsole uses the new port.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string"},
                    "host": {"type": "string"},
                    "port": {"type": "integer"},
                    "username": {"type": "string"},
                    "auth_type": {"type": "string", "description": "key or password"},
                    "key_path": {"type": "string"},
                    "name": {"type": "string"}
                },
                "required": []
            }),
        },
        ToolDef {
            name: "artifact_list".into(),
            description: "List files this agent created on the user's PC (SSH key backups, \
downloads, writes). Returns name, path, kind, size, sha256 — never private-key contents.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
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
first investigate with read-only tools, then call present_plan with the FULL plan in the required \
`plan` argument (title is optional). Never write the plan only as chat text — without this call the \
review modal does not open and the user cannot approve. The user can approve in the modal or in chat \
(you then execute) or request changes (you revise and present again). Blocks until the user responds."
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
server, to see the result of what you typed. Returns the tail of the screen; if the answer scrolled \
past, ask for more with max_chars rather than re-running the command. When several terminals are \
open on one server the reply names them — pass terminal_id to read a specific one.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string"},
                    "terminal_id": {"type": "string", "description": "Which terminal to read, when more than one is open for the server."},
                    "max_chars": {"type": "integer", "description": "How much scrollback to return (default 1800, max 40000)."}
                },
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
        ToolDef {
            name: "generate_svg".into(),
            description: "Generate a standalone vector graphic in SVG format (system architecture \
diagram, network topology, process flow, icon, or infographic). Validates XML and writes to \
artifacts/svg/ or the active workspace. Renders directly in the chat and on the canvas."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Short identifier/filename without extension, e.g. 'architecture_flow'"},
                    "svg_content": {"type": "string", "description": "The complete <svg ...>...</svg> XML content"},
                    "description": {"type": "string", "description": "Optional short summary of what this graphic represents"},
                    "target_dir": {"type": "string", "description": "Optional destination directory on local PC"}
                },
                "required": ["name", "svg_content"]
            }),
        },
        ToolDef {
            name: "generate_image".into(),
            description: "Generate an image or illustration from a detailed text prompt. Uses Flux / \
Pollinations by default (free, zero-API-key) or OpenAI DALL-E 3 if configured. Saves PNG to \
artifacts/images/ and renders inline."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "Detailed prompt describing the image, subject, lighting, and composition"},
                    "aspect_ratio": {"type": "string", "enum": ["1:1", "16:9", "9:16", "4:3", "3:2"], "description": "Image aspect ratio (default 1:1)"},
                    "name": {"type": "string", "description": "Optional short name for the output image file"}
                },
                "required": ["prompt"]
            }),
        },
        ToolDef {
            name: "explore".into(),
            description: "Send a sub-agent to find something out and report back in a few lines. \
It gets its own context and read-only tools, so the searching, the false starts and the files it \
opens never reach your conversation — only the answer does.\n\nUse it whenever finding out will \
take several steps and you only need the conclusion: \"where is the checkout handler and what \
calls it\", \"which service writes to this log\", \"is this config the one nginx actually \
loads\". Do not use it for something one grep would answer, or for anything that has to change \
something — it cannot."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "What you need to know, in full. It cannot see your conversation, so include what it needs: which project, which server, what you have already ruled out."},
                    "vps_id": {"type": "string", "description": "Server to look on. Defaults to the ones selected for this turn."}
                },
                "required": ["question"]
            }),
        },
        ToolDef {
            name: "canvas_open_preview".into(),
            description: "Open an interactive live HTML/CSS/JS preview or UI sandbox node directly on \
the xConsole canvas flow. Use this to demonstrate web components, designs, dashboards, or prototypes to the user."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "Title of the preview window (e.g. 'Hero Banner Component')"},
                    "html": {"type": "string", "description": "The complete HTML/CSS/JS code to render in the sandbox iframe"},
                    "width": {"type": "integer", "description": "Suggested width in px (default 800)"},
                    "height": {"type": "integer", "description": "Suggested height in px (default 600)"}
                },
                "required": ["title", "html"]
            }),
        },
    ];
    defs.extend(web_tools::definitions());
    defs.extend(infra_tools::definitions());
    defs.extend(crate::ai::persona_tools::definitions());
    defs.extend(crate::ai::remote_tools::definitions());
    defs.extend(crate::ai::metrics_tools::definitions());
    defs.extend(crate::ai::repo::definitions());
    defs.extend(crate::ai::transcript::definitions());
    defs
}

const OLLAMA_VPS_TOOLS: &[&str] = &[
    "run_command",
    "run_command_all",
    "list_vps_targets",
    "read_file",
    "write_file",
    "edit_file",
    "grep_search",
    // A local model hits the 120s foreground timeout as often as a frontier one, and
    // has less idea what to do about it. `find_files` also keeps it from grepping the
    // whole disk when it only needs to locate a config by name.
    "find_files",
    "job_status",
    "job_list",
    "job_kill",
    // Reading a file you had to guess the path of is guesswork twice. list_dir was
    // missing while local_list_dir was not, so a local model could look around this
    // PC but had to grope for paths on the server it was actually working on.
    "list_dir",
    "explore",
    "todo_write",
    "upload_file",
    "download_file",
    "ssh_setup_key_auth",
    "ssh_key_status",
    "vps_update_login",
    "artifact_list",
    "local_run_command",
    "local_read_file",
    "local_edit_file",
    "local_grep_search",
    "local_write_file",
    "local_list_dir",
    "terminal_send",
    "terminal_capture",
    "canvas_open_terminal",
    "canvas_open_sftp",
    "canvas_open_preview",
    "canvas_tile",
    "canvas_close",
    "canvas_refresh",
    "generate_svg",
    "generate_image",
    "ask_user",
    "present_plan",
    "rename_session",
    "memory_save",
    "host_memory_get",
    "host_memory_update",
    "taste_save",
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
    "local_edit_file",
    "local_grep_search",
    "local_write_file",
    "local_list_dir",
    "local_find_files",
    // local_run_command can start a detached job. Without these it could start one
    // and then had no way on earth to find out whether it worked.
    "job_status",
    "job_list",
    "job_kill",
    "explore",
    "artifact_list",
    "todo_write",
    "canvas_open_preview",
    "generate_svg",
    "generate_image",
    "ask_user",
    "present_plan",
    "rename_session",
    "memory_save",
    "taste_save",
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

pub async fn dispatch_with_telemetry(
    ctx: &ToolContext,
    call: &ToolCall,
    sink: &EventSink,
    telemetry: Option<&crate::ai::tool_cache::TurnTelemetryHandle>,
) -> String {
    if let Some(telemetry) = telemetry {
        crate::ai::tool_cache::record_tool_call(telemetry);
    }
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
    let scope = crate::ai::tool_cache::CacheScope::new(
        &ctx.session_id,
        ctx.workspace_id.as_deref(),
        &ctx.targets,
        &ctx.home.0,
    );

    // Short-TTL cache for read-only tools / web lookups (same scoped args → skip re-exec).
    let cacheable = crate::ai::tool_cache::is_cacheable(&call.name);
    if let Some(hit) = crate::ai::tool_cache::get_scoped(&scope, &call.name, args) {
        if let Some(telemetry) = telemetry {
            crate::ai::tool_cache::record_cache_lookup(telemetry, true);
        }
        emit(
            Some(sink),
            StreamEvent::Status(format!("Cache hit · {}", call.name)),
        );
        emit(
            Some(sink),
            StreamEvent::Activity(ActivityEvent::ToolEnd {
                id: call.id.clone(),
                ok: true,
            }),
        );
        return hit;
    }
    if cacheable {
        if let Some(telemetry) = telemetry {
            crate::ai::tool_cache::record_cache_lookup(telemetry, false);
        }
    }

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
    let plan_blocked = ctx.plan_mode
        && !ctx.session_state.plan_approved(&ctx.session_id)
        && tool_is_mutating(&call.name, args);
    let result = if ctx.read_only && tool_is_mutating(&call.name, args) {
        format!(
            "error: '{}' changes something, and you were asked a question. Report what you \
             found and let the agent that asked decide what to do about it.",
            call.name
        )
    } else if plan_blocked {
        format!(
            "error: plan mode is active. Investigate with read-only tools, then call present_plan \
             with your plan and wait for the user to approve it before running '{}'.",
            call.name
        )
    } else {
        match call.name.as_str() {
        "run_command" => run_command(ctx, args, sink, &call.id).await,
        "job_status" => job_status(ctx, args).await,
        "job_list" => job_list(ctx, args).await,
        "job_kill" => job_kill(ctx, args).await,
        "run_command_all" => run_command_all(ctx, args, sink, &call.id).await,
        "list_vps_targets" => list_vps_targets(ctx),
        "read_file" => read_file(ctx, args, sink, &call.id).await,
        "write_file" => write_file(ctx, args, sink, &call.id).await,
        "edit_file" => edit_file(ctx, args, sink, &call.id).await,
        "grep_search" => grep_search(ctx, args, sink, &call.id).await,
        "list_dir" => list_dir(ctx, args, sink).await,
        "find_files" => find_files(ctx, args, sink).await,
        "todo_write" => todo_write(ctx, args),
        "local_run_command" => local_run_command(ctx, args).await,
        "local_read_file" => local_read_file(ctx, args).await,
        "local_edit_file" => local_edit_file(ctx, args).await,
        "local_grep_search" => local_grep_search(ctx, args).await,
        "local_write_file" => local_write_file(ctx, args).await,
        "local_find_files" => local_find_files(ctx, args).await,
        "local_list_dir" => local_list_dir(ctx, args).await,
        "upload_file" => upload_file(ctx, args, sink, &call.id).await,
        "download_file" => download_file(ctx, args, sink, &call.id).await,
        "ssh_setup_key_auth" => ssh_setup_key_auth(ctx, args).await,
        "ssh_key_status" => ssh_key_status(ctx, args),
        "vps_update_login" => vps_update_login(ctx, args),
        "artifact_list" => artifact_list(ctx, args),
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
        "rename_session" => rename_session(ctx, args),
        "goal_propose_spec" => goal_propose_spec(ctx, args).await,
        "goal_add_task" => goal_add_task(ctx, args).await,
        "goal_update_task" => goal_update_task(ctx, args).await,
        "goal_record_constraint" => goal_record_constraint(ctx, args).await,
        "goal_check_criteria" => goal_check_criteria(ctx, args).await,
        "goal_schedule_wait" => goal_schedule_wait(ctx, args).await,
        "host_memory_get" => host_memory_get(ctx, args),
        "host_memory_update" => host_memory_update(ctx, args),
        "taste_save" => taste_save(ctx, args),
        "skills_list" => skills_list(ctx),
        "skill_view" => skill_view(ctx, args),
        "skill_save" => skill_save(ctx, args).await,
        "learn_skill" => learn_skill(ctx, args, sink).await,
        "generate_svg" => generate_svg_tool(ctx, args).await,
        "generate_image" => generate_image_tool(ctx, args).await,
        "explore" => explore(ctx, args, sink).await,
        "canvas_open_preview" => canvas_open_preview(ctx, args),
        name if web_tools::is_web_tool(name) => {
            // Gate `web_fetch` like a command. Its URL is model-controlled, so with a
            // file read in front of it this is a complete read-then-exfiltrate chain —
            // reachable from prompt injection in a page it fetched, or in output it read
            // off a server. The SSRF guard only blocks private/metadata addresses;
            // arbitrary public hosts are allowed by design, which is precisely the risk.
            // The system prompt asks the model to be careful here, but prose is not a
            // control. Search and geo hit fixed endpoints and stay ungated.
            let gate = if name == "web_fetch" {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
                authorize_local(ctx, &format!("web_fetch {url}")).await
            } else {
                Ok(())
            };
            match gate {
                Ok(()) => web_tools::dispatch(name, args).await,
                Err(e) => format!("error: {e}"),
            }
        }
        name if name.starts_with("project_")
            || name.starts_with("terraform_")
            || name.starts_with("cloud_")
            || name.starts_with("tfc_")
            || name.starts_with("cloudflare_") =>
        {
            infra_tools::dispatch(ctx, call.name.as_str(), args, sink).await
        }
        "vision" => vision_tool(ctx, args).await,
        n if crate::ai::persona_tools::is_persona_tool(n) => {
            crate::ai::persona_tools::dispatch(ctx, n, args).await
        }
        n if crate::ai::remote_tools::is_remote_tool(n) => {
            crate::ai::remote_tools::dispatch(ctx, n, args).await
        }
        n if crate::ai::metrics_tools::is_metric_tool(n) => {
            crate::ai::metrics_tools::dispatch(ctx, n, args).await
        }
        n if crate::ai::repo::is_repo_tool(n) => crate::ai::repo::dispatch(ctx, n, args).await,
        n if crate::ai::transcript::is_transcript_tool(n) => {
            crate::ai::transcript::dispatch(ctx, n, args).await
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
    if ok {
        invalidate_cache_after_success(&scope, &call.name, args);
        crate::ai::tool_cache::put_scoped(&scope, &call.name, args, &result);
        if cacheable {
            if let Some(telemetry) = telemetry {
                crate::ai::tool_cache::record_cache_write(telemetry);
            }
        }
    }
    emit(
        Some(sink),
        StreamEvent::Activity(ActivityEvent::ToolEnd {
            id: call.id.clone(),
            ok,
        }),
    );
    result
}

fn invalidate_cache_after_success(scope: &crate::ai::tool_cache::CacheScope, name: &str, args: &Value) {
    let cache = |invalidation: &crate::ai::tool_cache::Invalidation| {
        crate::ai::tool_cache::invalidate_scoped(scope, invalidation);
    };
    match name {
        "host_memory_update" => {
            if let Some(vps_id) = args.get("vps_id").and_then(Value::as_str) {
                cache(&crate::ai::tool_cache::Invalidation::HostMemory {
                    vps_id: vps_id.to_string(),
                });
            }
        }
        "skill_save" | "skill_install" | "learn_skill" => {
            cache(&crate::ai::tool_cache::Invalidation::Skills);
        }
        "local_write_file" | "download_file" => {
            if let Some(path) = args
                .get("path")
                .or_else(|| args.get("local_path"))
                .and_then(Value::as_str)
            {
                cache(&crate::ai::tool_cache::Invalidation::LocalFile {
                    path: path.to_string(),
                });
            }
        }
        "write_file" | "edit_file" | "upload_file" => {
            let vps_id = args.get("vps_id").and_then(Value::as_str);
            let path = args
                .get("path")
                .or_else(|| args.get("remote_path"))
                .and_then(Value::as_str);
            if let (Some(vps_id), Some(path)) = (vps_id, path) {
                cache(&crate::ai::tool_cache::Invalidation::RemoteFile {
                    vps_id: vps_id.to_string(),
                    path: path.to_string(),
                });
            }
        }
        _ => {}
    }
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
            if args.get("background").and_then(|v| v.as_bool()).unwrap_or(false) {
                format!("Start in background on {vps}: {cmd}")
            } else {
                format!("Run on {vps}: {cmd}")
            }
        }
        "agent_list" => "List named agents".into(),
        "agent_delegate" => format!(
            "Delegate to {}",
            args.get("agent").and_then(|v| v.as_str()).unwrap_or("an agent")
        ),
        "agent_check" => "Check delegated tasks".into(),
        "list_dir" => format!(
            "List {} on {}",
            args.get("path").and_then(|v| v.as_str()).unwrap_or("a directory"),
            vps_label(ctx, args)
        ),
        "find_files" => format!(
            "Find {} on {}",
            args.get("pattern").and_then(|v| v.as_str()).unwrap_or("files"),
            vps_label(ctx, args)
        ),
        "job_status" => format!(
            "Check background job {}",
            args.get("job_id").and_then(|v| v.as_str()).unwrap_or("…")
        ),
        "job_list" => format!("List background jobs on {}", vps_label(ctx, args)),
        "job_kill" => format!(
            "Stop background job {}",
            args.get("job_id").and_then(|v| v.as_str()).unwrap_or("…")
        ),
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
        "edit_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Edit {} on {}", path, vps_label(ctx, args))
        }
        "grep_search" => {
            let pat = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Search {pat} on {}", vps_label(ctx, args))
        }
        "todo_write" => "Update checklist".into(),
        "rename_session" => "Rename session".into(),
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
        "local_edit_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Edit {path} on this PC")
        }
        "local_grep_search" => {
            let pat = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("…");
            format!("Search {pat} on this PC")
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
        "vps_update_login" => format!("Update xConsole login for {}", vps_label(ctx, args)),
        "artifact_list" => "List local artifacts".into(),
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
        "vision" => {
            let n = args.get("image").and_then(|v| v.as_i64()).unwrap_or(1);
            format!("Look at image #{n}")
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
        | "artifact_list" | "ssh_key_status" | "memory_save" | "skills_list" | "skill_view"
        | "learn_skill" | "ask_user" | "present_plan" | "terminal_capture" | "canvas_open_terminal"
        | "canvas_open_sftp" | "canvas_open_preview" | "canvas_tile" | "canvas_close" | "canvas_refresh" | "vision"
        | "generate_svg" | "generate_image"
        | "grep_search" | "local_grep_search" | "find_files" | "todo_write" | "rename_session" => false,
        // Typing into a live shell runs commands → mutating.
        "terminal_send" => true,
        // Reading a job's state or log changes nothing on the host.
        "job_status" | "job_list" => false,
        // Signalling a process is a mutation, and plan mode must withhold it.
        "job_kill" => true,
        // Shell tools: mutating only when the command isn't read-only.
        "run_command" | "run_command_all" | "local_run_command" => {
            // Backgrounding is always a mutation, whatever the command does: it
            // leaves a detached process and its pid/log files behind on the host
            // and outlives the turn. Plan mode — where the user has said "don't do
            // anything yet" — must not let a `tail -f` be launched into the
            // background just because reading a log is read-only.
            if args.get("background").and_then(|v| v.as_bool()).unwrap_or(false) {
                return true;
            }
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            !safety::is_read_only(cmd)
        }
        // Always change a server or the local PC.
        "write_file" | "edit_file" | "local_write_file" | "local_edit_file"
        | "upload_file" | "download_file"
        | "ssh_setup_key_auth" | "vps_update_login" => true,
        // `web_fetch` reads nothing locally, but its URL is entirely model-chosen, so a
        // GET is an outbound channel: paired with a file read it can carry data off the
        // machine. Treating it as mutating is what makes plan mode — where the user has
        // explicitly said "don't do anything yet" — actually withhold it.
        "web_fetch" => true,
        // Search and geo hit fixed endpoints, so they can't be aimed at an attacker.
        n if web_tools::is_web_tool(n) => false,
        n if crate::ai::persona_tools::is_persona_tool(n) => {
            crate::ai::persona_tools::tool_is_mutating(n)
        }
        n if crate::ai::remote_tools::is_remote_tool(n) => {
            crate::ai::remote_tools::tool_is_mutating(n)
        }
        n if crate::ai::metrics_tools::is_metric_tool(n) => {
            crate::ai::metrics_tools::tool_is_mutating(n)
        }
        n if crate::ai::repo::is_repo_tool(n) => crate::ai::repo::tool_is_mutating(n),
        n if crate::ai::transcript::is_transcript_tool(n) => {
            crate::ai::transcript::tool_is_mutating(n)
        }
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
        // Asking a question changes nothing; the sub-agent it runs is read-only, and its
        // own guard refuses a further nesting.
        "explore" => false,
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

async fn vision_tool(ctx: &ToolContext, args: &Value) -> String {
    let index = args
        .get("image")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            args.get("image")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(1);
    let question = args
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let image = match crate::ai::vision::lookup_turn_image(&ctx.turn_images, index) {
        Ok(img) => img,
        Err(e) => return format!("error: {e}"),
    };
    match crate::ai::vision::describe_one(&ctx.db, image, &question).await {
        Ok(text) => text,
        Err(e) => format!("error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_command_keeps_a_hostile_pattern_as_one_argument() {
        // Patterns reach here from the model, i.e. from whatever it just read.
        let cmd = find_command("/etc", "-type f", "x'; rm -rf / ;'", 10);
        // POSIX close-escape-reopen: the whole thing stays one -name argument.
        assert!(cmd.contains(r"'x'\''; rm -rf / ;'\'''"), "{cmd}");
        assert!(!cmd.contains("-name x; rm"), "{cmd}");
    }

    #[test]
    fn find_command_quotes_the_search_root() {
        let cmd = find_command("/srv/a b; reboot", "-type f", "*.conf", 10);
        assert!(cmd.contains("'/srv/a b; reboot'"), "{cmd}");
    }

    #[test]
    fn find_command_prunes_the_expensive_trees_and_caps_results() {
        let cmd = find_command("/", "-type f", "*.service", 25);
        for pruned in ["/proc", "/sys", "/dev", "/run", "node_modules", ".git"] {
            assert!(cmd.contains(pruned), "missing prune for {pruned}: {cmd}");
        }
        assert!(cmd.contains("-prune -o"), "{cmd}");
        assert!(cmd.contains("head -n 25"), "{cmd}");
        // Escaped for the shell, so `find` itself receives real parentheses.
        assert!(cmd.contains(r"\("), "{cmd}");
        assert!(cmd.contains(r"\)"), "{cmd}");
    }

    #[test]
    fn find_command_can_look_for_directories() {
        assert!(find_command("/opt", "-type d", "conf.d", 5).contains("-type d"));
        // "any" passes no -type filter at all.
        assert!(!find_command("/opt", "", "conf.d", 5).contains("-type"));
    }

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
        assert!(!tool_is_mutating("vision", &json!({"image": 1, "question": "what"})));
        // Shell tools depend on whether the command is read-only.
        assert!(!tool_is_mutating("run_command", &json!({"command": "ls -la"})));
        assert!(tool_is_mutating("run_command", &json!({"command": "rm -rf /tmp/x"})));
        assert!(!tool_is_mutating("local_run_command", &json!({"command": "cat /etc/hosts"})));
        // Always-mutating tools.
        assert!(tool_is_mutating("write_file", &json!({})));
        assert!(tool_is_mutating("edit_file", &json!({})));
        assert!(!tool_is_mutating("grep_search", &json!({})));
        assert!(!tool_is_mutating("todo_write", &json!({})));
        assert!(tool_is_mutating("local_write_file", &json!({})));
        assert!(tool_is_mutating("upload_file", &json!({})));
        assert!(tool_is_mutating("ssh_setup_key_auth", &json!({})));
        assert!(tool_is_mutating("vps_update_login", &json!({})));
        assert!(!tool_is_mutating("artifact_list", &json!({})));
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

/// Authorize `shown` with the safety gate, then run `script`.
///
/// Backgrounding wraps the user's command in setsid/redirect/pid-file scaffolding.
/// Putting that wrapper in front of the user would make the approval prompt
/// unreadable, and would make an allowlist decision about `mkdir` rather than about
/// the command that actually runs — so the gate always sees the real command.
async fn exec_authorized_as(
    ctx: &ToolContext,
    vps_id: &str,
    shown: &str,
    script: &str,
) -> String {
    let base = safety::effective_mode(&ctx.db, &ctx.safety, vps_id);
    let mode = safety::resolve_session_mode(&ctx.session_state, &ctx.session_id, &base);
    if let Err(e) = safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &mode,
        &ctx.session_id,
        Some(vps_id),
        shown,
    )
    .await
    {
        return format!("error: {e}");
    }
    match ctx.sessions.run_command(vps_id, script).await {
        Ok(out) => {
            let text = out.stdout.trim();
            if out.exit_code != 0 {
                let err = out.stderr.trim();
                return format!("error (exit {}): {}", out.exit_code, if err.is_empty() { text } else { err });
            }
            text.to_string()
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn exec_inner(ctx: &ToolContext, vps_id: &str, command: &str) -> String {
    exec_inner_for(ctx, vps_id, command, crate::ssh::command::COMMAND_TIMEOUT).await
}

/// The same, with a deadline the caller chose.
async fn exec_inner_for(
    ctx: &ToolContext,
    vps_id: &str,
    command: &str,
    timeout: std::time::Duration,
) -> String {
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

    match ctx.sessions.run_command_for(vps_id, command, timeout).await {
        Ok(out) => format_command_output(&out),
        Err(e) => {
            let err = e.to_string();
            if is_connect_error(&err) {
                if let Some(jumped) = try_jump_command(ctx, vps_id, command).await {
                    return jumped;
                }
            }
            format!("error running command: {err}")
        }
    }
}

fn format_command_output(out: &crate::ssh::manager::CommandOutput) -> String {
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

fn is_connect_error(err: &str) -> bool {
    let l = err.to_lowercase();
    l.contains("could not reach")
        || l.contains("timed out")
        || l.contains("connection refused")
        || l.contains("no route")
        || l.contains("network is unreachable")
        || l.contains("forcibly closed")
        || l.contains("os error 10061")
        || l.contains("os error 10060")
        || l.contains("os error 10054")
}

/// When this PC cannot reach a host (firewall/reboot/lockout), hop through
/// another selected VPS that still answers. One hop only.
async fn try_jump_command(ctx: &ToolContext, dest_id: &str, command: &str) -> Option<String> {
    let dest = ctx.db.get_vps(dest_id).ok().flatten()?;
    for hop_id in &ctx.targets {
        if hop_id == dest_id {
            continue;
        }
        let hop = match ctx.db.get_vps(hop_id).ok().flatten() {
            Some(v) => v,
            None => continue,
        };
        let remote = format!(
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=8 -p {} {}@{} -- {}",
            dest.port,
            dest.username,
            dest.host,
            command
        );
        match ctx.sessions.run_command(hop_id, &remote).await {
            Ok(out) if out.exit_code == 0 || !out.stdout.trim().is_empty() => {
                return Some(format!(
                    "note: direct SSH to {} ({}) failed; jumped via {} ({}).\n{}",
                    dest.name,
                    dest.host,
                    hop.name,
                    hop.host,
                    format_command_output(&out)
                ));
            }
            _ => {}
        }
    }
    None
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
    if args.get("background").and_then(|v| v.as_bool()).unwrap_or(false) {
        return start_background_job(ctx, &vps_id, command).await;
    }
    let mut result = exec_inner_for(ctx, &vps_id, command, command_timeout(args)).await;
    result = crate::ai::vps_snapshot::annotate_command_output(command, &result);
    // The ToolResult is emitted once by the agent loop (run_command emits a
    // byte-identical one otherwise). Per-target/snapshot emits use distinct ids.
    result
}

/// Start `command` detached on the host and return its job id.
///
/// Goes through the same safety gate as a foreground command: backgrounding changes
/// when the agent sees the result, not what it is allowed to run.
async fn start_background_job(ctx: &ToolContext, vps_id: &str, command: &str) -> String {
    let job_id = jobs::new_job_id();
    let script = jobs::start_script(&job_id, command);
    // Authorize the *user's* command, not the wrapper — the approval prompt has to
    // show what will actually run, and an allowlist decision must be made about the
    // real command rather than the mkdir/setsid scaffolding around it.
    let out = exec_authorized_as(ctx, vps_id, command, &script).await;
    if out.starts_with("error") {
        return out;
    }
    format!(
        "Started background job {job_id} on this server.\n\
         The command keeps running after this turn; it is not affected by the 120s \
         foreground timeout.\n\
         Check progress with job_status(job_id: \"{job_id}\"). Do not wait idly — \
         continue with other work and check back."
    )
}


/// Start `command` detached on this PC and return its job id.
///
/// The server side has had this since jobs existed; the PC had neither it nor an
/// adjustable deadline, so a long local build had no recourse at all. The two use
/// different shells — `setsid sh -c` against `Start-Process -EncodedCommand` — but write
/// the same four files and print the same status contract, so nothing downstream has to
/// know which machine a job is on.
///
/// The authorisation is the same as running it in the foreground: backgrounding changes
/// how long it takes, not what it may do.
async fn start_local_background_job(command: &str) -> String {
    let job_id = jobs::new_job_id();
    let script = if cfg!(windows) {
        jobs::start_script_windows(&job_id, command)
    } else {
        jobs::start_script(&job_id, command)
    };
    match crate::local::run_local_command(&script).await {
        Ok(out) if out.stdout.contains("started") => format!(
            "Started background job {job_id} on this PC.\n\
             It keeps running after this turn ends. Check it with \
             job_status(job_id: \"{job_id}\", local: true), and stop it with job_kill."
        ),
        Ok(out) => format!(
            "error: could not start the job. {}",
            if out.stderr.trim().is_empty() { out.stdout } else { out.stderr }
        ),
        Err(e) => format!("error starting the job: {e}"),
    }
}

/// Whether a job call is about this PC rather than a server.
/// Whether a job tool is being asked about this PC rather than a server.
///
/// With no VPS selected there is nowhere else the job could be, so the flag is
/// assumed rather than demanded: otherwise a session working purely on the local
/// machine starts a job with `local_run_command` and then gets "no VPS target
/// selected" when it asks how the job went, which reads like the job is gone.
fn job_is_local(ctx: &ToolContext, args: &Value) -> bool {
    args.get("local")
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| ctx.targets.is_empty())
}

/// Run one of the job scripts where the job actually lives.
///
/// A job on this PC and a job on a server are the same idea with two shells behind it,
/// and both scripts print the same `STATE=`/`EXIT=`/`PID=` contract — so everything
/// that reads a job's status stays one implementation and only this chooses the shell.
async fn run_job_script(
    ctx: &ToolContext,
    args: &Value,
    posix: impl FnOnce() -> String,
    windows: impl FnOnce() -> String,
) -> Result<String, String> {
    if job_is_local(ctx, args) {
        // The PC's own shell decides: PowerShell on Windows, sh everywhere else.
        let script = if cfg!(windows) { windows() } else { posix() };
        return crate::local::run_local_command(&script)
            .await
            .map(|o| if o.stdout.trim().is_empty() { o.stderr } else { o.stdout });
    }
    let vps_id = resolve_target(ctx, args)?;
    ctx.sessions
        .run_command(&vps_id, &posix())
        .await
        .map(|o| o.stdout)
        .map_err(|e| e.to_string())
}

async fn job_status(ctx: &ToolContext, args: &Value) -> String {
    let job_id = match args.get("job_id").and_then(|v| v.as_str()) {
        Some(j) if jobs::is_valid_job_id(j) => j.to_string(),
        Some(j) => return format!("error: {j:?} is not a job id"),
        None => return "error: missing 'job_id'".into(),
    };
    let tail = args
        .get("tail_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(jobs::DEFAULT_TAIL_LINES as u64) as u32;
    // Reading a log is read-only, so it does not need the approval gate the job
    // itself already passed — asking again for every poll would defeat the point.
    let id = job_id.clone();
    let id2 = job_id.clone();
    match run_job_script(
        ctx,
        args,
        || jobs::status_script(&id, tail),
        || jobs::status_script_windows(&id2, tail),
    )
    .await
    {
        Ok(out) => {
            let text = out.trim().to_string();
            if text.contains("STATE=unknown") {
                let where_ = if job_is_local(ctx, args) { "this PC" } else { "this server" };
                format!("No job {job_id} on {where_} (it may have been started somewhere else, or the temp directory was cleared). Use job_list.")
            } else {
                text
            }
        }
        Err(e) => format!("error checking job {job_id}: {e}"),
    }
}

async fn job_list(ctx: &ToolContext, args: &Value) -> String {
    let where_ = if job_is_local(ctx, args) { "this PC" } else { "this server" };
    match run_job_script(ctx, args, jobs::list_script, jobs::list_script_windows).await {
        Ok(out) => {
            let text = out.trim();
            if text.is_empty() || text.contains("(no jobs)") {
                format!("No background jobs on {where_}.")
            } else {
                format!("Background jobs on {where_} (newest first):\n{text}")
            }
        }
        Err(e) => format!("error listing jobs: {e}"),
    }
}

async fn job_kill(ctx: &ToolContext, args: &Value) -> String {
    let job_id = match args.get("job_id").and_then(|v| v.as_str()) {
        Some(j) if jobs::is_valid_job_id(j) => j.to_string(),
        Some(j) => return format!("error: {j:?} is not a job id"),
        None => return "error: missing 'job_id'".into(),
    };
    // Stopping a job the user's agent started is a mutation, so it goes through the
    // safety gate like any other one — on either machine.
    if job_is_local(ctx, args) {
        let script = if cfg!(windows) {
            jobs::kill_script_windows(&job_id)
        } else {
            jobs::kill_script(&job_id)
        };
        if let Err(e) = authorize_local(ctx, &format!("stop background job {job_id}")).await {
            return format!("error: {e}");
        }
        return match crate::local::run_local_command(&script).await {
            Ok(out) => format_local_output(&out),
            Err(e) => format!("error stopping job {job_id}: {e}"),
        };
    }
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    exec_authorized_as(
        ctx,
        &vps_id,
        &format!("stop background job {job_id}"),
        &jobs::kill_script(&job_id),
    )
    .await
}

async fn run_command_all(ctx: &ToolContext, args: &Value, sink: &EventSink, id: &str) -> String {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return "error: missing 'command'".into(),
    };
    run_command_all_targets_impl(ctx, command, sink, id, command_timeout(args)).await
}

/// Run one command on every selected target (shared by tools and Ollama auto-collect).
pub async fn run_command_all_targets(
    ctx: &ToolContext,
    command: &str,
    sink: &EventSink,
) -> String {
    run_command_all_targets_impl(ctx, command, sink, "auto", crate::ssh::command::COMMAND_TIMEOUT)
        .await
}

async fn run_command_all_targets_impl(
    ctx: &ToolContext,
    command: &str,
    sink: &EventSink,
    activity_prefix: &str,
    timeout: std::time::Duration,
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
        let mut out = exec_inner_for(ctx, vps_id, command, timeout).await;
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

/// How long a caller is prepared to wait for one command.
///
/// Two minutes stays the default — most commands are quick, and a long default would
/// turn a hung one into a long silence. What changed is that it is no longer the
/// ceiling: a build or a migration that needs five is not a failure, and reporting it as
/// one taught the agent to split work that has to run in one go.
/// The wait for one command, from the model's `timeout_secs`.
///
/// Clamped to the hour the tool descriptions advertise. Uncapped, a fat-fingered
/// 360000 is not a long timeout, it is a hung session: the turn would sit on a
/// dead SSH read for days with no way for the agent to notice or recover.
fn command_timeout(args: &Value) -> std::time::Duration {
    const MAX_SECS: u64 = 3600;
    args.get("timeout_secs")
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .map(|n| std::time::Duration::from_secs(n.min(MAX_SECS)))
        .unwrap_or(crate::ssh::command::COMMAND_TIMEOUT)
}

/// Build the directory listing. Pure, so the quoting and the cap are testable.
///
/// `ls -l` rather than anything cleverer: it is on every machine, the agent reads it
/// without help, and a bespoke format would be one more thing to keep true. The `-A`
/// includes dotfiles but not `.` and `..`, which are noise in every listing.
pub fn list_dir_command(path: &str, all: bool, head: u64) -> String {
    let p = shell_quote(path);
    let a = if all { "A" } else { "" };
    format!(
        "d={p}; \
         if [ ! -d \"$d\" ]; then \
           if [ -e \"$d\" ]; then echo 'NOT_A_DIRECTORY'; else echo 'NOT_FOUND'; fi; exit 0; \
         fi; \
         ls -l{a}h --time-style=long-iso -- \"$d\" 2>/dev/null | head -n {head}; \
         echo \"__XCONS_TOTAL__ $(ls -{a}1 -- \"$d\" 2>/dev/null | wc -l)\""
    )
}

async fn list_dir(ctx: &ToolContext, args: &Value, sink: &EventSink) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.trim().is_empty() => p.trim(),
        _ => return "error: missing 'path'".into(),
    };
    let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(true);
    let head = args.get("head_limit").and_then(|v| v.as_u64()).unwrap_or(100).clamp(1, 500);
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let raw = exec(ctx, &vps_id, &list_dir_command(path, all, head), Some(sink), true).await;
    if raw.starts_with("error") {
        return raw;
    }
    let body = stdout_body(&raw);
    let trimmed = body.trim();
    // Told apart on purpose: "there is no such directory" and "there is a file by that
    // name" lead to completely different next steps, and an empty listing implies
    // neither.
    if trimmed.starts_with("NOT_FOUND") {
        return format!("{path} does not exist on that server.");
    }
    if trimmed.starts_with("NOT_A_DIRECTORY") {
        return format!("{path} is a file, not a directory — read it with read_file.");
    }

    let (listing, total) = match trimmed.rsplit_once("__XCONS_TOTAL__") {
        Some((l, t)) => (l.trim_end(), t.trim().parse::<u64>().unwrap_or(0)),
        None => (trimmed, 0),
    };
    if listing.is_empty() {
        return format!("{path} is empty.");
    }
    let shown = listing.lines().filter(|l| !l.starts_with("total ")).count() as u64;
    format!(
        "{path} ({total} entr{}):\n{listing}{}",
        if total == 1 { "y" } else { "ies" },
        if total > shown && shown > 0 {
            format!("\n\n(showing {shown} of {total} — raise head_limit for the rest)")
        } else {
            String::new()
        }
    )
}

/// The files a read was asked for: the `paths` array, or the single `path`.
///
/// Both spellings, for the same reason the edit tools keep both: opening one file
/// should not need a list, and opening five should not need five calls.
fn requested_paths(args: &Value) -> Result<Vec<String>, String> {
    if let Some(list) = args.get("paths").and_then(|v| v.as_array()) {
        let paths: Vec<String> = list
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        if paths.is_empty() {
            return Err("error: 'paths' was empty".into());
        }
        return Ok(paths);
    }
    match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.trim().is_empty() => Ok(vec![p.trim().to_string()]),
        _ => Err("error: give either 'path' or a non-empty 'paths' array".into()),
    }
}

async fn read_file(ctx: &ToolContext, args: &Value, sink: &EventSink, id: &str) -> String {
    let paths = match requested_paths(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if paths.len() > 1 {
        return read_many(ctx, args, sink, id, &paths).await;
    }
    let path = paths[0].as_str();
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let command = format!(
        "stat -c %Y -- {p} 2>/dev/null; echo __XCONS_MTIME__; cat -- {p}",
        p = shell_quote(path)
    );
    let raw = exec(ctx, &vps_id, &command, Some(sink), true).await;
    if raw.starts_with("error") {
        return raw;
    }
    let (mtime, body) = split_mtime_prefix(&raw);
    let file = stdout_body(&body);
    let offset = args.get("offset").and_then(|v| v.as_i64());
    let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32);
    let numbered = crate::ai::file_ops::format_read(&file, offset, limit);
    let detected_encoding = crate::ai::file_ops::detect_encoding(file.as_bytes());
    if let Some(m) = mtime {
        crate::ai::file_state::note_read_with_encoding(
            &ctx.session_id,
            &vps_id,
            path,
            &m,
            Some(detected_encoding),
        );
        if detected_encoding != "utf-8" {
            format!("[mtime: {m}, encoding: {detected_encoding}]\n{numbered}")
        } else {
            format!("[mtime: {m}]\n{numbered}")
        }
    } else {
        numbered
    }
}

/// Read several files in one SSH round trip.
///
/// Each read is a full round trip, so opening a config, the file it includes and the
/// unit that starts it was three of them — and that is the ordinary shape of finding
/// something out, not an unusual one. One command, separated by a marker.
///
/// Offset and limit are deliberately not accepted here: they would apply to every file
/// at once, which is never what anyone means. Read the batch to see what is there, then
/// read one file with a window.
async fn read_many(
    ctx: &ToolContext,
    args: &Value,
    sink: &EventSink,
    _id: &str,
    paths: &[String],
) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    // A cap, because "read the whole directory" through this would be one enormous
    // result that the tool-result budget then truncates from the end — losing the last
    // files entirely rather than any of them partially.
    const MAX_FILES: usize = 20;
    let paths = &paths[..paths.len().min(MAX_FILES)];

    const SEP: &str = "__XCONS_FILE__";
    let script = paths
        .iter()
        .map(|p| {
            let q = shell_quote(p);
            // The mtime rides along, so a later edit can still refuse to clobber a file
            // that changed since it was read.
            format!("echo '{SEP}'; echo {q}; stat -c %Y -- {q} 2>/dev/null || echo 0; cat -- {q} 2>&1")
        })
        .collect::<Vec<_>>()
        .join("; ");

    let raw = exec(ctx, &vps_id, &script, Some(sink), true).await;
    if raw.starts_with("error") {
        return raw;
    }
    let body = stdout_body(&raw);

    let mut out = String::new();
    for chunk in body.split(SEP).skip(1) {
        let mut lines = chunk.trim_start_matches('\n').splitn(3, '\n');
        let Some(path) = lines.next() else { continue };
        let mtime = lines.next().unwrap_or("0").trim();
        let content = lines.next().unwrap_or("");
        if mtime.chars().all(|c| c.is_ascii_digit()) && mtime != "0" {
            crate::ai::file_state::note_read_with_encoding(
                &ctx.session_id,
                &vps_id,
                path,
                mtime,
                Some(crate::ai::file_ops::detect_encoding(content.as_bytes())),
            );
        }
        out.push_str(&format!(
            "\n===== {path} =====\n{}\n",
            crate::ai::file_ops::format_read(content, None, None)
        ));
    }
    if out.is_empty() {
        return format!("error: read nothing back for {} file(s)", paths.len());
    }
    format!("{} file(s):{out}", paths.len())
}

fn stdout_body(wrapped: &str) -> String {
    wrapped
        .split_once("stdout:\n")
        .map(|(_, rest)| {
            rest.split("\nstderr:\n").next().unwrap_or(rest).to_string()
        })
        .unwrap_or_else(|| wrapped.to_string())
}

fn split_mtime_prefix(raw: &str) -> (Option<String>, String) {
    // exec wraps stdout after "stdout:\n"
    let stdout = raw
        .split_once("stdout:\n")
        .map(|(_, rest)| rest)
        .unwrap_or(raw);
    let Some((head, tail)) = stdout.split_once("__XCONS_MTIME__") else {
        return (None, raw.to_string());
    };
    let mtime = head
        .lines()
        .rev()
        .find(|l| l.chars().all(|c| c.is_ascii_digit()))
        .map(|s| s.to_string());
    let body = tail.trim_start_matches('\n');
    // Rebuild keeping the exit_code header if present.
    if let Some((hdr, _)) = raw.split_once("stdout:\n") {
        (mtime, format!("{hdr}stdout:\n{body}"))
    } else {
        (mtime, body.to_string())
    }
}

async fn write_file(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => normalize_vps_write_path(p),
        _ => return "error: missing 'path'".into(),
    };
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let mode = WriteMode::parse(args);
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    // An append adds to the end and replaces nothing, so a file that grew since the
    // read is not a reason to refuse it — that check exists to stop work being
    // overwritten, and there is nothing here to overwrite.
    if mode == WriteMode::Overwrite {
        if let Some(e) = check_remote_write(ctx, &vps_id, &path).await {
            return e;
        }
    }
    // Capture the file's current content first so the changes panel can diff it.
    let before = ctx
        .sessions
        .run_command(&vps_id, &format!("cat -- {}", shell_quote(&path)))
        .await
        .map(|o| o.stdout)
        .unwrap_or_default();
    // The bytes go out through the same function edit_file uses. They used to be
    // two copies of the same base64 pipeline, which is how the two of them can
    // drift on encoding, on the diff record, or on re-stamping the mtime.
    write_remote_contents(ctx, &vps_id, &path, content, sink, &before, mode).await
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

/// Refuse a write whose file has moved on disk since this session read it.
///
/// One copy, called by both `write_file` and `edit_file`. They had a block each,
/// which is how one of them ends up with the check and the other without it.
async fn check_remote_write(ctx: &ToolContext, vps_id: &str, path: &str) -> Option<String> {
    let st = ctx
        .sessions
        .run_command(vps_id, &format!("stat -c %Y -- {} 2>/dev/null", shell_quote(path)))
        .await
        .ok()?;
    let current = st
        .stdout
        .lines()
        .find(|l| l.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("");
    crate::ai::file_state::check_write(&ctx.session_id, vps_id, path, current).err()
}

/// How a write meets a file that already has something in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WriteMode {
    Overwrite,
    Append,
}

impl WriteMode {
    fn parse(args: &Value) -> Self {
        match args.get("mode").and_then(|v| v.as_str()) {
            Some("append") => WriteMode::Append,
            _ => WriteMode::Overwrite,
        }
    }
}

async fn write_remote_contents(
    ctx: &ToolContext,
    vps_id: &str,
    path: &str,
    content: &str,
    sink: &EventSink,
    before: &str,
    mode: WriteMode,
) -> String {
    let parent = std::path::Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|p| !p.is_empty())
        .unwrap_or("/tmp");
    let target_encoding = crate::ai::file_state::get_encoding(&ctx.session_id, vps_id, path)
        .unwrap_or_else(|| "utf-8".to_string());
    let encoded_bytes = crate::ai::file_ops::encode_text_with_charset(content, &target_encoding);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&encoded_bytes);
    // The stat is part of the write, not a second round trip: it both confirms the
    // bytes actually landed — an exit code alone does not tell you the file has the
    // size you meant — and hands back the new mtime for the re-stamp below.
    //
    // It cannot be allowed to fail the command, though. BusyBox and the BSDs have no
    // `stat -c`, and chaining it with a bare `&&` there would turn a write that
    // worked into a reported failure on exactly the small boxes people run.
    let redirect = match mode {
        WriteMode::Overwrite => ">",
        WriteMode::Append => ">>",
    };
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d {redirect} {} && \
         {{ stat -c 'xc-wrote %Y %s' -- {} 2>/dev/null || true ; }}",
        shell_quote(parent),
        shell_quote(&b64),
        shell_quote(path),
        shell_quote(path)
    );
    let result = exec(ctx, vps_id, &command, Some(sink), true).await;
    if !result.starts_with("exit_code: 0") {
        return result;
    }
    let (mtime, size) = parse_stat_line(&result);
    // Re-stamp, or the agent's own write looks like somebody else's the next time
    // it touches this file.
    crate::ai::file_state::note_write(&ctx.session_id, vps_id, path, &mtime);
    let label = ctx
        .db
        .get_vps(vps_id)
        .ok()
        .flatten()
        .map(|v| format!("{} ({})", v.name, v.host))
        .unwrap_or_else(|| vps_id.to_string());
    // What the file now says, not just what this call sent — otherwise the changes
    // panel shows an append as the whole file being replaced by the new chunk.
    let after = match mode {
        WriteMode::Overwrite => content.to_string(),
        WriteMode::Append => format!("{before}{content}"),
    };
    ctx.edits.record(
        &ctx.app,
        &ctx.session_id,
        ctx.workspace_id.clone(),
        "vps",
        Some(vps_id.to_string()),
        &label,
        path,
        before,
        &after,
    );
    let sent = encoded_bytes.len();
    // An append lands on top of what was already there, so the file is rightly
    // longer than this write. Comparing against `sent` alone would report every
    // successful append as a size mismatch.
    let expected = match mode {
        WriteMode::Overwrite => sent,
        WriteMode::Append => sent + before.len(),
    };
    let verb = match mode {
        WriteMode::Overwrite => "wrote",
        WriteMode::Append => "appended",
    };
    match size {
        Some(n) if n == expected as u64 => format!("{verb} {sent} bytes to {path} (size verified)"),
        Some(n) => format!(
            "warning: {verb} {sent} bytes to {path} but it is {n} bytes on disk, not the \
             {expected} expected. Read it back before you rely on it."
        ),
        None => format!("{verb} {sent} bytes to {path} (size could not be read back)"),
    }
}

/// The sha256 out of a trailing `sha256sum` line, if the server had the command.
///
/// `sha256sum` prints `<hex>  <path>`. Anything else — "command not found", a
/// path containing spaces, a BSD-style `SHA256 (f) = ...` — yields None rather
/// than a guess, because a wrong hash here would condemn a good upload.
fn remote_sha256(output: &str) -> Option<String> {
    output.lines().rev().find_map(|line| {
        let first = line.split_whitespace().next()?;
        (first.len() == 64 && first.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| first.to_lowercase())
    })
}

/// Pull `mtime` and `size` out of the trailing `stat -c '%Y %s'` line of a write.
///
/// Kept separate from the SSH call so the parsing is testable: the line arrives at
/// the end of whatever else the command printed, and a server whose `stat` is
/// missing or differently spelled must degrade to "unknown", never to a wrong
/// number that would be reported to the user as a verified write.
fn parse_stat_line(output: &str) -> (String, Option<u64>) {
    for line in output.lines().rev() {
        let Some(rest) = line.trim().strip_prefix("xc-wrote ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let (Some(a), Some(b), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if let (Ok(_), Ok(size)) = (a.parse::<u64>(), b.parse::<u64>()) {
            return (a.to_string(), Some(size));
        }
    }
    (String::new(), None)
}

/// The edits an edit tool was asked for: the `edits` array, or the single-edit form.
///
/// Both spellings are kept because both are the natural one for their case — a one-line
/// fix should not need an array, and a five-place change should not need five calls.
fn requested_edits(args: &Value) -> Result<Vec<(String, String, bool)>, String> {
    if let Some(list) = args.get("edits").and_then(|v| v.as_array()) {
        if list.is_empty() {
            return Err("error: 'edits' was empty".into());
        }
        return list
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let old = e.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                if old.is_empty() {
                    return Err(format!("error: edit {} has no old_string", i + 1));
                }
                Ok((
                    old.to_string(),
                    e.get("new_string").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    e.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false),
                ))
            })
            .collect();
    }
    let old = args.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    if old.is_empty() {
        return Err("error: give either 'old_string' or a non-empty 'edits' array".into());
    }
    Ok(vec![(
        old.to_string(),
        args.get("new_string").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false),
    )])
}

/// Run the requested edits over one file's contents.
fn edit_contents(before: &str, args: &Value) -> Result<(String, usize, usize), String> {
    let requested = requested_edits(args)?;
    let edits: Vec<crate::ai::file_ops::Edit<'_>> = requested
        .iter()
        .map(|(o, n, all)| crate::ai::file_ops::Edit { old: o, new: n, replace_all: *all })
        .collect();
    let (next, n) = crate::ai::file_ops::apply_edits(before, &edits)?;
    Ok((next, n, edits.len()))
}

async fn edit_file(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => normalize_vps_write_path(p),
        _ => return "error: missing 'path'".into(),
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    if let Err(e) = authorize_vps(ctx, &vps_id, &format!("edit {path}")).await {
        return format!("error: {e}");
    }
    let before_out = match ctx
        .sessions
        .run_command(&vps_id, &format!("cat -- {}", shell_quote(&path)))
        .await
    {
        Ok(o) => o,
        Err(e) => return format!("error: {e}"),
    };
    if before_out.exit_code != 0 && before_out.stdout.is_empty() {
        return format!(
            "error: could not read {path} (exit {}). Use write_file to create it.",
            before_out.exit_code
        );
    }
    if let Some(e) = check_remote_write(ctx, &vps_id, &path).await {
        return e;
    }
    let (next, n, count) = match edit_contents(&before_out.stdout, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let result =
        write_remote_contents(ctx, &vps_id, &path, &next, sink, &before_out.stdout, WriteMode::Overwrite)
            .await;
    if result.starts_with("wrote ") {
        format!(
            "updated {path} ({n} replacement{} from {count} edit{}). {result}",
            if n == 1 { "" } else { "s" },
            if count == 1 { "" } else { "s" }
        )
    } else {
        result
    }
}

/// Build the `find` invocation used by [`find_files`].
///
/// Split out so the quoting can be tested: `path` and `pattern` come from the model,
/// which means they come from whatever the model has read — a filename on a server,
/// a web page, a tool result. A pattern like `x; rm -rf /` has to stay one argument.
///
/// The prune list skips the pseudo-filesystems and dependency caches that otherwise
/// dominate a walk from `/`: they are most of the cost and never hold the config or
/// unit file being looked for.
fn find_command(path: &str, kind: &str, pattern: &str, head: u64) -> String {
    format!(
        "find {path} \\( -path /proc -o -path /sys -o -path /dev -o -path /run \
         -o -path '*/node_modules' -o -path '*/.git' \\) -prune -o \
         {kind} -name {pat} -print 2>/dev/null | head -n {head}",
        path = shell_quote(path),
        pat = shell_quote(pattern),
    )
}

/// Find files by name. The name-shaped counterpart to [`grep_search`].
///
/// Without this the only way to locate a file whose name is known but whose path is
/// not was to grep the filesystem for its contents — slow on a real server, and it
/// misses the file entirely when the name is the only thing known about it.
async fn find_files(ctx: &ToolContext, args: &Value, sink: &EventSink) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'pattern'".into(),
    };
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("/");
    let head = args
        .get("head_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_FIND_RESULTS)
        .clamp(1, MAX_FIND_RESULTS);
    let kind = match args.get("type").and_then(|v| v.as_str()).unwrap_or("file") {
        "dir" => "-type d",
        "any" => "",
        _ => "-type f",
    };
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let cmd = find_command(path, kind, pattern, head);
    let raw = exec(ctx, &vps_id, &cmd, Some(sink), true).await;
    let body = stdout_body(&raw);
    let body = body.trim_end();
    if body.trim().is_empty() {
        format!("no files matching {pattern:?} under {path}")
    } else {
        let n = body.lines().count();
        let capped = if n as u64 >= head {
            format!("\n(stopped at {head} results — narrow `path` or `pattern` for the rest)")
        } else {
            String::new()
        };
        format!("{n} match(es) for {pattern:?} under {path}:\n{body}{capped}")
    }
}

/// What a search should return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepOutput {
    /// Matching lines, optionally with surrounding context.
    Content,
    /// Just the file names, for "where does this live" questions.
    Files,
    /// One count per file, for "how widespread is this".
    Count,
}

impl GrepOutput {
    fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("content") {
            "files" | "files_with_matches" => GrepOutput::Files,
            "count" => GrepOutput::Count,
            _ => GrepOutput::Content,
        }
    }
}

/// Build the search command, preferring ripgrep and falling back to grep.
///
/// Pure, because the bug it was written to fix was invisible from reading it: ripgrep's
/// `-m` caps matches *per file*, not overall, so a pattern common across five hundred
/// files returned thousands of lines. The tool-result cap then truncated that to an
/// arbitrary slice — the agent saw a fraction of the answer and no sign that it had.
/// Both branches are now capped overall by `head`.
/// Everything a search is, in one place.
///
/// Seven positional arguments was already one too many to read at a call site, and
/// the offset and multiline flags would have made nine. A struct also means adding
/// the next option does not touch every caller and every test.
#[derive(Debug, Clone)]
pub struct GrepArgs<'a> {
    pub pattern: &'a str,
    pub path: &'a str,
    pub glob: Option<&'a str>,
    pub case_insensitive: bool,
    pub head: u64,
    pub offset: u64,
    pub context: u64,
    pub multiline: bool,
    pub mode: GrepOutput,
}

impl<'a> GrepArgs<'a> {
    /// A plain content search with everything else defaulted.
    ///
    /// Only the tests build searches this way — the dispatchers set every field
    /// from the model's arguments — so it is compiled with them.
    #[cfg(test)]
    pub fn new(pattern: &'a str, path: &'a str) -> Self {
        Self {
            pattern,
            path,
            glob: None,
            case_insensitive: false,
            head: DEFAULT_GREP_LINES,
            offset: 0,
            context: 0,
            multiline: false,
            mode: GrepOutput::Content,
        }
    }
}

/// How many matching lines a search returns when nobody says otherwise.
///
/// It was 40, and hard-capped at 200 however much was asked for. That is small
/// enough that an ordinary search across a repository is truncated most times it
/// runs, and with no offset there was no way to see what came after — so the
/// answer to "where else is this called" was reliably cut off mid-list, and the
/// only recourse was to narrow the search until it fitted, which is guesswork.
pub const DEFAULT_GREP_LINES: u64 = 250;
/// The ceiling on one search. High enough not to be met by accident, low enough
/// that a pattern matching everything cannot bury a turn.
pub const MAX_GREP_LINES: u64 = 2000;

/// How many paths a name search returns by default, and at most.
///
/// Also 40 and capped at 200 before. A repository has more than 200 source files,
/// so "find every *.rs" quietly answered with some of them.
pub const DEFAULT_FIND_RESULTS: u64 = 200;
pub const MAX_FIND_RESULTS: u64 = 2000;

pub fn grep_command(a: &GrepArgs) -> String {
    let GrepArgs {
        pattern,
        path,
        glob,
        case_insensitive,
        head,
        offset,
        context,
        multiline,
        mode,
    } = *a;
    let pat = shell_quote(pattern);
    let dir = shell_quote(path);
    let i = if case_insensitive { " -i" } else { "" };
    let rg_glob = glob.map(|g| format!(" -g {}", shell_quote(g))).unwrap_or_default();
    // `find -name` for the fallback: plain grep has no glob filter of its own.
    let grep_glob = glob
        .map(|g| format!(" --include={}", shell_quote(g)))
        .unwrap_or_default();

    let (rg_mode, grep_mode) = match mode {
        // Context only makes sense around content.
        GrepOutput::Content if context > 0 => (format!(" -n -C {context}"), format!(" -n -C {context}")),
        GrepOutput::Content => (" -n".to_string(), " -n".to_string()),
        GrepOutput::Files => (" -l".to_string(), " -l".to_string()),
        GrepOutput::Count => (" -c".to_string(), " -c".to_string()),
    };
    // Per-file cap as well, so one enormous file cannot fill the whole budget on its own
    // and hide every other file's matches.
    let per_file = match mode {
        GrepOutput::Content => format!(" -m {}", head.max(1)),
        _ => String::new(),
    };

    // Skip before capping, so a truncated search can be continued rather than only
    // narrowed. `tail -n +N` counts from 1, hence the +1.
    let window = if offset > 0 {
        format!("tail -n +{} | head -n {head}", offset + 1)
    } else {
        format!("head -n {head}")
    };
    // Patterns that span lines are rg-only; plain grep has no equivalent, and
    // pretending otherwise would silently return the wrong answer on a box without
    // rg rather than saying it could not do it.
    let ml = if multiline { " -U --multiline-dotall" } else { "" };
    let fallback = if multiline {
        "echo 'error: multiline search needs ripgrep, which is not installed on this host'".to_string()
    } else {
        format!("grep -R -E{grep_mode}{i}{grep_glob} -- {pat} {dir} 2>/dev/null | {window}")
    };

    format!(
        "if command -v rg >/dev/null 2>&1; then \
           rg --no-heading --color never{rg_mode}{ml}{i}{per_file}{rg_glob} -e {pat} -- {dir} 2>/dev/null | {window}; \
         else \
           {fallback}; \
         fi"
    )
}

/// What the result is, so a list of file names is not read as a list of matches.
fn grep_header(mode: GrepOutput, context: u64) -> String {
    match mode {
        GrepOutput::Files => "files containing it".to_string(),
        GrepOutput::Count => "matches per file".to_string(),
        GrepOutput::Content if context > 0 => format!("matches with {context} line(s) of context"),
        GrepOutput::Content => "matches (path:line:text)".to_string(),
    }
}

/// Said when the output hit the cap, because a silently truncated list reads as the
/// whole answer — and the agent then concludes something is not there when it is.
fn grep_note(body: &str, head: u64, offset: u64) -> String {
    if body.lines().count() as u64 >= head {
        // Say how to continue. Telling the agent only that the output stopped
        // leaves narrowing the pattern as the sole way forward, and narrowing a
        // search whose remainder you cannot see is guessing.
        format!(
            "\n\n(stopped at {head} lines. For the next {head} call grep_search again with \
             offset: {}, or raise head_limit)",
            offset + head
        )
    } else {
        String::new()
    }
}

async fn grep_search(ctx: &ToolContext, args: &Value, sink: &EventSink, _id: &str) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'pattern'".into(),
    };
    let path = args.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("/");
    let glob = args.get("glob").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
    let head = args
        .get("head_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_GREP_LINES)
        .clamp(1, MAX_GREP_LINES);
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
    let multiline = args.get("multiline").and_then(|v| v.as_bool()).unwrap_or(false);
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let context = args.get("context_lines").and_then(|v| v.as_u64()).unwrap_or(0).min(20);
    let mode = GrepOutput::parse(args.get("output_mode").and_then(|v| v.as_str()));
    let cmd = grep_command(&GrepArgs {
        pattern,
        path,
        glob,
        case_insensitive: ci,
        head,
        offset,
        context,
        multiline,
        mode,
    });
    let raw = exec(ctx, &vps_id, &cmd, Some(sink), true).await;
    let body = stdout_body(&raw);
    if body.trim().is_empty() {
        return format!("no matches for {pattern:?} under {path}");
    }
    format!(
        "{}:\n{}{}",
        grep_header(mode, context),
        body.trim_end(),
        grep_note(&body, head, offset)
    )
}

/// Answer one question in a context of its own, and hand back only the answer.
///
/// Finding something out is expensive in the wrong currency: a search that reads ten
/// files puts ten file dumps into the conversation, and they stay there for every turn
/// afterwards. The answer was five lines. On a long session that is the difference
/// between a thread that still fits and one that has to be compacted, losing the early
/// reasoning that made the later work make sense.
///
/// So this runs a real turn with a fresh session id — its own context, nothing shared —
/// and read-only tools, and returns its final message. The false starts stay in there.
///
/// Read-only is not only about safety. An agent that can act on what it finds will act,
/// and then the caller has a change it did not ask for and did not see happen.
async fn explore(ctx: &ToolContext, args: &Value, sink: &EventSink) -> String {
    let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("").trim();
    if question.is_empty() {
        return "error: explore needs a 'question'".into();
    }
    // No sub-agent spawning a sub-agent. The depth is never worth it — a question that
    // needs delegating again was too broad — and unbounded recursion here is a way to
    // spend a lot of money on nothing.
    if ctx.read_only {
        return "error: you are already answering a question for another agent. Investigate \
                it yourself and report what you find."
            .into();
    }

    let targets = match args.get("vps_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(v) => vec![v.to_string()],
        None => ctx.targets.clone(),
    };

    let sub = ToolContext {
        app: ctx.app.clone(),
        db: ctx.db.clone(),
        sessions: ctx.sessions.clone(),
        home: ctx.home.clone(),
        approvals: ctx.approvals.clone(),
        // Nothing it does needs approving, because nothing it does changes anything —
        // and there is nobody watching a sub-turn to answer a prompt anyway.
        prompts: crate::ai::interaction::PromptRegistry::new(),
        // Its own state as well as its own id: the caller's todos, plan approval and
        // prefix cache are not its business, and it must not be able to disturb them.
        session_state: crate::ai::interaction::SessionState::new(),
        session_id: format!("explore:{}", uuid::Uuid::new_v4()),
        targets,
        safety: ctx.safety.clone(),
        plan_mode: false,
        read_only: true,
        workspace_id: ctx.workspace_id.clone(),
        canvas: Vec::new(),
        edits: ctx.edits.clone(),
        hooks: ctx.hooks.clone(),
        turn_images: Vec::new(),
        goal_id: None,
        persona_id: ctx.persona_id.clone(),
    };

    let prompt = format!(
        "Another agent needs to know one thing and has sent you to find out. You have \
         read-only tools; you cannot change anything, and you are not being asked to.\n\n\
         Investigate properly — grep, read the files, follow it through — and then answer \
         in a few lines. Cite paths and line numbers so the answer can be checked. If you \
         could not find out, say what you looked at and what you would try next; a wrong \
         confident answer is worse than an admission, because it will be acted on.\n\n\
         Do not describe your search, do not summarise your process, and do not offer to \
         do more. Just the answer.\n\nThe question:\n{question}"
    );

    emit(Some(sink), StreamEvent::Status(format!("Looking into: {question}")));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    // Its stream is its own. Forwarding it would put back into the conversation exactly
    // the noise this exists to keep out — but errors are worth surfacing, since an
    // exploration that failed silently reads as an exploration that found nothing.
    let outer = sink.clone();
    let drain = tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let StreamEvent::Error(e) = &ev {
                let _ = outer.send(StreamEvent::Status(format!("(exploring: {e})")));
            }
        }
    });

    // Boxed: `explore` runs a turn, and a turn can dispatch `explore`, so the future
    // refers to itself. The depth guard above stops it happening at run time; this is
    // what lets the type check.
    let result = Box::pin(crate::ai::agent::run_turn(
        &sub,
        crate::ai::registry::ModelChoice::active(),
        vec![crate::ai::provider::ChatMessage::user(prompt)],
        false,
        &tx,
    ))
    .await;
    drop(tx);
    let _ = drain.await;

    match result {
        Ok(msg) if !msg.content.trim().is_empty() => msg.content,
        Ok(_) => "The sub-agent finished without answering. Ask it again more narrowly, or \
                  look yourself."
            .to_string(),
        Err(e) => format!("The sub-agent could not finish: {e}"),
    }
}

fn todo_write(ctx: &ToolContext, args: &Value) -> String {
    let Some(arr) = args.get("todos").and_then(|v| v.as_array()) else {
        return "error: missing 'todos' array".into();
    };
    let parsed: Vec<crate::ai::todos::TodoItem> = arr
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    let items = crate::ai::todos::normalize_list(parsed);
    if items.is_empty() {
        return "error: no valid todo items".into();
    }
    ctx.session_state.set_todos(&ctx.session_id, items.clone());
    crate::ai::todos::format_activity(&items)
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

fn rename_session(ctx: &ToolContext, args: &Value) -> String {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
    if title.is_empty() {
        return "error: missing or empty 'title'".into();
    }
    if let Ok(Some(conv)) = ctx.db.get_agent_conversation(&ctx.session_id) {
        let targets: Vec<String> = conv
            .targets_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        let input = crate::storage::models::AgentConversationInput {
            id: conv.id,
            title: Some(title.to_string()),
            targets,
            messages_json: conv.messages_json,
        };
        let _ = ctx.db.upsert_agent_conversation(&input);
    }
    format!("Session renamed to: {title}")
}

fn host_memory_get(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = args.get("vps_id").and_then(|v| v.as_str()).unwrap_or("");
    if vps_id.is_empty() {
        return "error: missing 'vps_id'".into();
    }
    if !ctx.targets.iter().any(|t| t == vps_id) {
        return "error: vps_id is not in the selected targets for this turn".into();
    }
    if !ctx.db.get_vps(vps_id).ok().flatten().is_some() {
        return "error: VPS target was not found".into();
    }
    let profile = crate::ai::host_memory::load_profile(&ctx.home, vps_id);
    let mem = crate::ai::host_memory::load_memory(&ctx.home, vps_id);
    if profile.trim().is_empty() && mem.trim().is_empty() {
        return format!("(no dossier yet for {vps_id} — update with host_memory_update)");
    }
    let mut out = String::new();
    if !profile.trim().is_empty() {
        out.push_str("# PROFILE\n");
        out.push_str(profile.trim());
        out.push('\n');
    }
    if !mem.trim().is_empty() {
        out.push_str("\n# MEMORY\n");
        out.push_str(mem.trim());
    }
    out
}

fn host_memory_update(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = args.get("vps_id").and_then(|v| v.as_str()).unwrap_or("");
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if vps_id.is_empty() {
        return "error: missing 'vps_id'".into();
    }
    if content.trim().is_empty() {
        return "error: missing 'content'".into();
    }
    if !ctx.targets.iter().any(|t| t == vps_id) {
        return "error: vps_id is not in the selected targets for this turn".into();
    }
    if !ctx.db.get_vps(vps_id).ok().flatten().is_some() {
        return "error: VPS target was not found".into();
    }
    match kind {
        "profile" => match crate::ai::host_memory::save_profile(&ctx.home, vps_id, content) {
            Ok(()) => "saved host PROFILE".into(),
            Err(e) => format!("error: {e}"),
        },
        "memory" => match crate::ai::host_memory::append_memory(&ctx.home, vps_id, content) {
            Ok(_) => "appended to host MEMORY".into(),
            Err(e) => format!("error: {e}"),
        },
        _ => "error: kind must be 'profile' or 'memory'".into(),
    }
}

fn taste_save(ctx: &ToolContext, args: &Value) -> String {
    let entry = args.get("entry").and_then(|v| v.as_str()).unwrap_or("");
    if entry.trim().is_empty() {
        return "error: missing 'entry'".into();
    }
    match crate::ai::taste::append(&ctx.home, entry) {
        Ok(_) => "saved to TASTE.md".into(),
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

    let report = match skill_scan::scan_skill_content(
        &skill_md,
        &skill_scan::scan_options_from_db(&ctx.db),
    )
    .await
    {
        Ok(report) => report,
        Err(e) => return format!("error: scanning skill: {e}"),
    };

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
    //
    // Gate on the RAW text. Prefixing it with prose used to hand the safety check a
    // string whose leading token was "type" — which is on the read-only allowlist — so
    // `is_read_only` approved the sentence rather than the command, and anything at all
    // (`rm -rf /var/www`) sailed through unattended in allowlist mode.
    if let Err(e) = authorize_vps(ctx, &vps_id, text).await {
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
    let Some(first) = sessions.first() else {
        return "error: no terminal is open on the canvas for that server.".into();
    };
    // With several terminals open on one server, reading whichever came back first
    // and saying nothing is how you report the output of the wrong session as if it
    // were the one you typed into. Take the named one when asked, and always say
    // which was read.
    let sid = match args.get("terminal_id").and_then(|v| v.as_str()) {
        Some(want) if !want.is_empty() => match sessions.iter().find(|s| *s == want) {
            Some(s) => s,
            None => {
                return format!(
                    "error: no terminal {want} is open for that server. Open ones: {}",
                    sessions.join(", ")
                )
            }
        },
        _ => first,
    };
    let others = if sessions.len() > 1 {
        format!(
            "[terminal {sid} — {} others open on this server: {}. Pass terminal_id to read one of them.]\n",
            sessions.len() - 1,
            sessions.iter().filter(|s| *s != sid).cloned().collect::<Vec<_>>().join(", ")
        )
    } else {
        String::new()
    };
    let text = ctx.sessions.capture_text(sid).unwrap_or_default();
    let trimmed = text.trim_end();
    // Prefix with live-canvas git branch when the frontend has reported it for a
    // terminal on this VPS (avoids an extra SSH round-trip in the tool).
    let git_note = ctx
        .canvas
        .iter()
        .find(|n| {
            n.kind == "terminal"
                && n.vps_id == vps_id
                && n.git_branch
                    .as_deref()
                    .map(|b| !b.is_empty())
                    .unwrap_or(false)
        })
        .map(|n| {
            let br = n.git_branch.as_deref().unwrap_or("?");
            let dirty = n.git_dirty.unwrap_or(false);
            format!(
                "[git: {br}{}]\n",
                if dirty { " (dirty)" } else { "" }
            )
        })
        .unwrap_or_default();
    // Return the tail (recent screen) to keep it compact. A build that failed 300
    // lines up is invisible in 1800 characters, so the cap is liftable — the agent
    // asks for more only when the tail did not contain the answer.
    let cap = args
        .get("max_chars")
        .and_then(|v| v.as_u64())
        .map(|n| n.clamp(200, 40_000) as usize)
        .unwrap_or(1800);
    let body = if trimmed.len() > cap {
        let start = trimmed.len() - cap;
        let cut = (start..trimmed.len())
            .find(|&i| trimmed.is_char_boundary(i))
            .unwrap_or(start);
        format!("…(earlier output trimmed)\n{}", &trimmed[cut..])
    } else if trimmed.is_empty() {
        "(terminal is empty)".into()
    } else {
        trimmed.to_string()
    };
    format!("{others}{git_note}{body}")
}

/// Emit a canvas action to the frontend (open/close a node). Resolves the VPS the
/// same way other tools do so it stays within the selected targets.
fn canvas_command_tool(ctx: &ToolContext, args: &Value, action: &str) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    let label = ctx
        .db
        .get_vps(&vps_id)
        .ok()
        .flatten()
        .map(|v| v.name)
        .unwrap_or_else(|| vps_id.clone());
    if action == "open_terminal" {
        let already = ctx
            .canvas
            .iter()
            .any(|n| n.kind == "terminal" && n.vps_id == vps_id);
        let live = !ctx.sessions.live_sessions_for_vps(&vps_id).is_empty();
        if already || live {
            let _ = ctx.app.emit(
                "canvas://command",
                json!({ "action": "open_terminal", "vps_id": vps_id }),
            );
            return format!(
                "a terminal for {label} is already on the canvas — focused it. \
                 Do not call canvas_open_terminal again for this host; use terminal_send \
                 or run_command."
            );
        }
    }
    let _ = ctx.app.emit(
        "canvas://command",
        json!({ "action": action, "vps_id": vps_id }),
    );
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

async fn skill_save(ctx: &ToolContext, args: &Value) -> String {
    let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if category.trim().is_empty() || name.trim().is_empty() || content.trim().is_empty() {
        return "error: category, name, and non-empty content are required".into();
    }
    let report = match skill_scan::scan_skill_content(
        content,
        &skill_scan::scan_options_from_db(&ctx.db),
    )
    .await
    {
        Ok(report) => report,
        Err(e) => return format!("error: scanning skill: {e}"),
    };
    if report.is_blocking() {
        return format!("BLOCKED: skill was not saved.\n{}", report.summary());
    }
    let gate = format!(
        "save unverified skill '{category}/{name}' (scan: {} risk {}/100, scanner {})",
        report.severity, report.risk_score, report.scanner
    );
    if let Err(e) = authorize_local(ctx, &gate).await {
        return format!("error: {e}");
    }
    match skills::save_unverified(&ctx.home, name, content) {
        Ok(saved) => format!("saved unverified skill unverified/{saved}; promote it after review.\n{}", report.summary()),
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

    let scan_opts = skill_scan::scan_options_from_db(&ctx.db);
    let result = crate::ai::autoresearch::learn(
        &ctx.home,
        resolved.provider.as_ref(),
        &resolved.model,
        topic,
        name_hint,
        &known_hosts,
        None,
        &scan_opts,
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
    if args.get("background").and_then(|v| v.as_bool()).unwrap_or(false) {
        return start_local_background_job(command).await;
    }
    match crate::local::run_local_command_for(command, command_timeout(args)).await {
        Ok(out) => format_local_output(&out),
        Err(e) => format!("error running command: {e}"),
    }
}

async fn local_read_file(ctx: &ToolContext, args: &Value) -> String {
    let paths = match requested_paths(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Several files reads the same way as it does on a server. Not for the round trip —
    // there is none here — but because an agent should not have to remember which of two
    // tools takes a list, and the secret checks below then apply to every file rather
    // than only the first.
    if paths.len() > 1 {
        let mut out = String::new();
        for p in paths.iter().take(20) {
            let one = serde_json::json!({ "path": p });
            out.push_str(&format!(
                "\n===== {p} =====\n{}\n",
                Box::pin(local_read_file(ctx, &one)).await
            ));
        }
        return format!("{} file(s):{out}", paths.len().min(20));
    }
    let path = paths[0].as_str();
    // Gate as a read (allowlisted, so it auto-runs under allowlist mode).
    if let Err(e) = authorize_local(ctx, &format!("cat -- {}", shell_quote(path))).await {
        return format!("error: {e}");
    }
    if crate::artifacts::looks_like_secret_path(path) {
        return "error: refused — that path looks like a private key or app secret. \
I cannot read passwords or private keys. Use artifact_list for the backup path/hash."
            .into();
    }
    match crate::local::read_local_file(path) {
        Ok(s) => {
            if crate::artifacts::looks_like_private_key_content(s.as_bytes()) {
                "error: refused — file is a private key. I cannot read key material. \
The user can open it from Settings → Artifacts or the saved path."
                    .into()
            } else {
                // Stamp the read, the same way the server twin does. Without this a
                // later local write has nothing to compare against and clobbers
                // silently — and this PC is the machine with the git checkouts on it.
                crate::ai::file_state::note_read_with_encoding(
                    &ctx.session_id,
                    crate::ai::file_state::LOCAL,
                    path,
                    &crate::ai::file_state::local_mtime(path),
                    Some(crate::ai::file_ops::detect_encoding(s.as_bytes())),
                );
                let offset = args.get("offset").and_then(|v| v.as_i64());
                let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32);
                crate::ai::file_ops::format_read(&s, offset, limit)
            }
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn local_edit_file(ctx: &ToolContext, args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };

    if let Err(e) = authorize_local(ctx, &format!("edit local file {path}")).await {
        return format!("error: {e}");
    }
    if let Err(e) = crate::ai::file_state::check_write(
        &ctx.session_id,
        crate::ai::file_state::LOCAL,
        path,
        &crate::ai::file_state::local_mtime(path),
    ) {
        return e;
    }
    let before = match crate::local::read_local_file(path) {
        Ok(s) => s,
        Err(e) => return format!("error: {e}"),
    };
    let (next, n, _count) = match edit_contents(&before, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match crate::artifacts::write_verified(std::path::Path::new(path), next.as_bytes()) {
        Ok(written) => {
            crate::ai::file_state::note_write(
                &ctx.session_id,
                crate::ai::file_state::LOCAL,
                path,
                &crate::ai::file_state::local_mtime(path),
            );
            ctx.edits.record(
                &ctx.app,
                &ctx.session_id,
                ctx.workspace_id.clone(),
                "local",
                None,
                "This PC",
                path,
                &before,
                &next,
            );
            format!(
                "updated {path} ({n} replacement{}, {} bytes, sha256 {})",
                if n == 1 { "" } else { "s" },
                written.size,
                written.sha256
            )
        }
        Err(e) => format!("error: {e}"),
    }
}

/// The same search as `grep_search`, run here instead of over SSH.
///
/// Built by the same function, deliberately. This was a near-copy of it, and the copy
/// drifted the way copies do: it had no plain-grep fallback at all, so local search
/// simply failed on a machine without ripgrep; it kept the per-file `-m` that let one
/// large file crowd out every other result; and it never gained context lines or output
/// modes. Every one of those was fixed once, in the other copy.
async fn local_grep_search(ctx: &ToolContext, args: &Value) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'pattern'".into(),
    };
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    let glob = args.get("glob").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
    let head = args
        .get("head_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_GREP_LINES)
        .clamp(1, MAX_GREP_LINES);
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
    let multiline = args.get("multiline").and_then(|v| v.as_bool()).unwrap_or(false);
    let context = args.get("context_lines").and_then(|v| v.as_u64()).unwrap_or(0).min(20);
    let mode = GrepOutput::parse(args.get("output_mode").and_then(|v| v.as_str()));

    let cmd = grep_command(&GrepArgs {
        pattern,
        path,
        glob,
        case_insensitive: ci,
        head,
        offset,
        context,
        multiline,
        mode,
    });
    if let Err(e) = authorize_local(ctx, &cmd).await {
        return format!("error: {e}");
    }
    match crate::local::run_local_command(&cmd).await {
        Ok(out) => {
            let text = if out.stdout.trim().is_empty() { out.stderr.clone() } else { out.stdout.clone() };
            if text.trim().is_empty() {
                return format!("no matches for {pattern:?} under {path}");
            }
            format!("{}:\n{}{}", grep_header(mode, context), text.trim_end(), grep_note(&text, head, offset))
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn local_write_file(ctx: &ToolContext, args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let mode = WriteMode::parse(args);
    if let Err(e) = authorize_local(ctx, &format!("write local file {path}")).await {
        return format!("error: {e}");
    }
    // See write_file: an append replaces nothing, so a file that grew is not a
    // reason to refuse it.
    if mode == WriteMode::Overwrite {
        if let Err(e) = crate::ai::file_state::check_write(
            &ctx.session_id,
            crate::ai::file_state::LOCAL,
            path,
            &crate::ai::file_state::local_mtime(path),
        ) {
            return e;
        }
    }
    let before = crate::local::read_local_file(path).ok();
    let target_encoding = crate::ai::file_ops::detect_encoding(before.as_deref().unwrap_or("").as_bytes());
    let whole = match mode {
        WriteMode::Overwrite => content.to_string(),
        WriteMode::Append => format!("{}{content}", before.as_deref().unwrap_or("")),
    };
    let encoded_bytes = crate::ai::file_ops::encode_text_with_charset(&whole, target_encoding);
    match crate::artifacts::write_verified(std::path::Path::new(path), &encoded_bytes) {
        Ok(written) => {
            crate::ai::file_state::note_write(
                &ctx.session_id,
                crate::ai::file_state::LOCAL,
                path,
                &crate::ai::file_state::local_mtime(path),
            );
            ctx.edits.record(
                &ctx.app,
                &ctx.session_id,
                ctx.workspace_id.clone(),
                "local",
                None,
                "This PC",
                path,
                before.as_deref().unwrap_or(""),
                // The whole file as it now stands: an append recorded as `content`
                // alone shows in the changes panel as the file being replaced by
                // the chunk that was added to it.
                &whole,
            );
            record_artifact(
                ctx,
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path),
                &written.path.to_string_lossy(),
                "file",
                &written.sha256,
                written.size,
                crate::artifacts::looks_like_private_key_content(whole.as_bytes()),
                None,
            );
            format!(
                "wrote {} bytes to {path} (sha256 {} — verified)",
                written.size, written.sha256
            )
        }
        Err(e) => format!("error: {e}"),
    }
}

/// The twin of `find_files`, run here rather than over SSH.
///
/// Built by the same `find_command`, so the pruning of /proc, node_modules and .git,
/// the newest-first ordering and the result cap are the same on both — the divergence
/// that had crept into the two search implementations is not worth repeating here.
async fn local_find_files(ctx: &ToolContext, args: &Value) -> String {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(p) if !p.trim().is_empty() => p.trim(),
        _ => return "error: missing 'pattern'".into(),
    };
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    let kind = match args.get("type").and_then(|v| v.as_str()).unwrap_or("file") {
        "dir" => "-type d",
        "any" => "",
        _ => "-type f",
    };
    let head = args
        .get("head_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_FIND_RESULTS)
        .clamp(1, MAX_FIND_RESULTS);

    let cmd = find_command(path, kind, pattern, head);
    if let Err(e) = authorize_local(ctx, &cmd).await {
        return format!("error: {e}");
    }
    match crate::local::run_local_command(&cmd).await {
        Ok(out) => {
            let body = out.stdout.trim();
            if body.is_empty() {
                return format!("no files matching {pattern:?} under {path}");
            }
            let n = body.lines().count() as u64;
            format!(
                "{n} match(es), newest first:\n{body}{}",
                if n >= head {
                    format!("\n\n(stopped at {head} — narrow the path or raise head_limit)")
                } else {
                    String::new()
                }
            )
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

/// Read a remote file as base64, refusing on the server anything over the limit.
///
/// The order is the point, and it is why this is a function rather than a string
/// inline: the limit used to be applied after the whole file had crossed the wire
/// as base64 and been decoded in memory, so a download refused for being too large
/// was one already paid for in full — a 2GB file arriving as ~2.7GB of text in RAM
/// before anything said no. Asking costs one `wc -c`.
///
/// A missing file falls through to `base64`, which fails with the real error rather
/// than a size complaint about a file that is not there.
fn download_command(remote_path: &str) -> String {
    format!(
        "sz=$(wc -c < {p} 2>/dev/null || echo -1) ; \
         if [ \"$sz\" -gt {MAX_TRANSFER} ] ; then echo \"xc-too-big $sz\" ; else base64 -- {p} ; fi",
        p = shell_quote(remote_path)
    )
}

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
    // Check what landed. This is the binary path — a tarball, a dump, a compiled
    // artifact — where a truncated transfer is both the likeliest failure and the
    // one an exit code says nothing about. Reporting "uploaded 4.2 MB" because the
    // shell returned 0 is a claim with nothing behind it, and the user finds out
    // when the archive will not open.
    //
    // Neither check may fail the upload: plenty of small boxes have no `stat -c`
    // and no `sha256sum`, and there the honest answer is that it could not be
    // confirmed, not that it did not work.
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {} && \
         {{ stat -c 'xc-wrote %Y %s' -- {} 2>/dev/null || true ; }} && \
         {{ sha256sum -- {} 2>/dev/null || true ; }}",
        shell_quote(parent),
        shell_quote(&b64),
        shell_quote(remote_path),
        shell_quote(remote_path),
        shell_quote(remote_path)
    );

    let activity_id = format!("upload-{vps_id}");
    emit_command_activity(ctx, sink, &activity_id, &vps_id, &format!("upload → {remote_path}"));
    let sent = bytes.len();
    let want = crate::artifacts::sha256_hex(&bytes);
    let result = match ctx.sessions.run_command(&vps_id, &command).await {
        Ok(out) if out.exit_code == 0 => {
            let (_, size) = parse_stat_line(&out.stdout);
            let got = remote_sha256(&out.stdout);
            match (size, got) {
                (Some(n), _) if n != sent as u64 => format!(
                    "error: upload landed short — {n} bytes on the server, {sent} sent. \
                     {remote_path} is corrupt; delete it and try again."
                ),
                (_, Some(h)) if h != want => format!(
                    "error: upload arrived corrupted — sha256 on the server is {h}, \
                     the file sent was {want}. Do not use {remote_path}."
                ),
                (_, Some(_)) => format!("uploaded {sent} bytes to {remote_path} (sha256 verified)"),
                (Some(_), None) => format!("uploaded {sent} bytes to {remote_path} (size verified)"),
                (None, None) => format!(
                    "uploaded {sent} bytes to {remote_path} (the server has neither stat nor \
                     sha256sum, so this could not be checked)"
                ),
            }
        }
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
    // See download_command: the size is asked before anything is pulled. The limit used to be applied after the
    // whole file had been base64'd across the wire and decoded in memory, so a
    // download refused for being too large was one that had already been paid for
    // in full — a 2GB file arriving as ~2.7GB of text in RAM before anything said
    // no. Refusing costs one `wc -c` now.
    let read_cmd = download_command(remote_path);
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
    if let Some(rest) = out.stdout.trim().strip_prefix("xc-too-big ") {
        let m = format!(
            "error: {remote_path} is {} bytes, over the {MAX_TRANSFER} limit — nothing was \
             transferred. Copy a part of it, or compress it on the server first.",
            rest.trim()
        );
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
    let result = match crate::artifacts::write_verified(std::path::Path::new(local_path), &bytes) {
        Ok(written) => {
            record_artifact(
                ctx,
                std::path::Path::new(local_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(local_path),
                &written.path.to_string_lossy(),
                "download",
                &written.sha256,
                written.size,
                crate::artifacts::looks_like_private_key_content(&bytes),
                Some(vps_id.clone()),
            );
            format!(
                "downloaded {} bytes to {local_path} (sha256 {} — verified)",
                written.size, written.sha256
            )
        }
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
    let save_backup = args
        .get("save_backup")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let backup_dir = if !save_backup {
        None
    } else if let Some(custom) = args.get("backup_dir").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(std::path::PathBuf::from(custom))
    } else {
        ctx.app.path().app_data_dir().ok().and_then(|d| {
            let vps = ctx.db.get_vps(&vps_id).ok().flatten()?;
            Some(crate::artifacts::ssh_backup_dir(&d, &vps_id, &vps.name))
        })
    };
    match keygen::setup_key_auth(&ctx.db, &ctx.sessions, &vps_id, backup_dir.as_deref()).await {
        Ok(r) => {
            let _ = ctx.app.emit("vps://updated", &vps_id);
            let _ = ctx.app.emit("artifacts://changed", ());
            let mut out = format!(
                "Key authentication is set up.\nFingerprint: {}\nInstalled public key: {}\n\
                 Password login on the server was left enabled as a fallback.\n\
                 The private key is in the OS keychain (never returned here).",
                r.fingerprint, r.public_openssh
            );
            if r.backup_verified {
                out.push_str(&format!(
                    "\nVerified local backup: {}\nPublic key file: {}\nSHA-256: {}\n\
                     xConsole now uses this key file for this server. Open Settings → Artifacts to find it.",
                    r.backup_private_path.unwrap_or_default(),
                    r.backup_public_path.unwrap_or_default(),
                    r.backup_sha256.unwrap_or_default()
                ));
            } else if let Some(msg) = r.backup_private_path {
                out.push_str(&format!("\nLocal backup: {msg}"));
            }
            out
        }
        Err(e) => format!("error: {e}"),
    }
}

fn vps_update_login(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = match resolve_target(ctx, args) {
        Ok(id) => id,
        Err(e) => return format!("error: {e}"),
    };
    if args.get("password").is_some() || args.get("secret").is_some() || args.get("private_key").is_some()
    {
        return "error: refused — I cannot set or read passwords or private keys. \
Use ssh_setup_key_auth to create a key, then vps_update_login for host/port/username/key_path."
            .into();
    }
    let auth_type = args.get("auth_type").and_then(|v| v.as_str()).map(|s| {
        crate::storage::models::AuthType::from_str(s)
    });
    let port = args.get("port").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_i64().map(|n| n as u64))
            .and_then(|n| u16::try_from(n).ok())
    });
    let patch = crate::storage::models::VpsLoginPatch {
        name: args.get("name").and_then(|v| v.as_str()).map(String::from),
        host: args.get("host").and_then(|v| v.as_str()).map(String::from),
        port,
        username: args.get("username").and_then(|v| v.as_str()).map(String::from),
        auth_type,
        key_path: args.get("key_path").and_then(|v| v.as_str()).map(String::from),
    };
    match ctx.db.patch_vps_login(&vps_id, &patch) {
        Ok(v) => {
            let _ = ctx.app.emit("vps://updated", &v.id);
            format!(
                "Updated xConsole login (no secrets exposed).\n\
                 vps_id: {}\nname: {}\nhost: {}\nport: {}\nusername: {}\nauth_type: {}\nkey_path: {}",
                v.id,
                v.name,
                v.host,
                v.port,
                v.username,
                v.auth_type.as_str(),
                v.key_path.unwrap_or_else(|| "(managed key / none)".into())
            )
        }
        Err(e) => format!("error: {e}"),
    }
}

fn artifact_list(ctx: &ToolContext, args: &Value) -> String {
    let query = args.get("query").and_then(|v| v.as_str());
    match ctx.db.list_artifacts(query) {
        Ok(list) if list.is_empty() => "(no artifacts yet)".into(),
        Ok(list) => list
            .into_iter()
            .map(|a| {
                format!(
                    "- [{}] {}  {} bytes  sha256={}  path={}{}",
                    a.kind,
                    a.name,
                    a.size,
                    a.sha256,
                    a.path,
                    if a.secret { "  (secret — contents hidden)" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("error: {e}"),
    }
}

fn record_artifact(
    ctx: &ToolContext,
    name: &str,
    path: &str,
    kind: &str,
    sha256: &str,
    size: u64,
    secret: bool,
    vps_id: Option<String>,
) {
    let art = crate::artifacts::Artifact {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        path: path.to_string(),
        kind: kind.to_string(),
        sha256: sha256.to_string(),
        size,
        secret,
        session_id: Some(ctx.session_id.clone()),
        vps_id,
        created_at: None,
    };
    if ctx.db.insert_artifact(&art).is_ok() {
        let _ = ctx.app.emit("artifacts://changed", ());
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

/// Cron and goal runs are unattended — there is nobody to answer interactive
/// prompts, and their private registries can't be resolved from the UI (which
/// would deadlock the turn for the full PROMPT_TIMEOUT).
fn is_unattended_session(session_id: &str) -> bool {
    session_id.starts_with("cron:") || session_id.starts_with("goal:")
}

async fn ask_user(ctx: &ToolContext, args: &Value) -> String {
    if is_unattended_session(&ctx.session_id) {
        return "error: ask_user is not available in unattended runs (cron/goal)".into();
    }
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
    let rx = ctx.prompts.register_for_session(id.clone(), &ctx.session_id);
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
    if is_unattended_session(&ctx.session_id) {
        return "error: plan mode is not available in unattended runs (cron/goal). Present the \
                plan as normal text instead, and do not attempt to execute anything."
            .into();
    }
    let plan = match crate::ai::consent::plan_body_from_args(args) {
        Some(p) => p,
        None => {
            return "error: missing 'plan' — pass the full markdown plan in the `plan` argument \
                    (aliases: content, text, steps)."
                .into()
        }
    };
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Plan");
    let id = Uuid::new_v4().to_string();
    let stored = crate::storage::models::AgentPlan {
        id: id.clone(),
        session_id: ctx.session_id.clone(),
        workspace_id: ctx.workspace_id.clone(),
        title: Some(title.to_string()),
        plan: plan.to_string(),
        status: "presented".into(),
        created_at: None,
        updated_at: None,
    };
    let _ = ctx.db.insert_agent_plan(&stored);
    let payload = json!({
        "id": id,
        "session_id": ctx.session_id,
        "workspace_id": ctx.workspace_id,
        "title": title,
        "plan": plan,
    });
    let _ = ctx.app.emit("ai://plan", payload);
    // A new presentation supersedes any still-pending one for this session.
    ctx.prompts.cancel_superseded(&ctx.session_id, &id);
    let rx = ctx.prompts.register_for_session(id.clone(), &ctx.session_id);
    match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
        Ok(Ok(decision)) => {
            // The frontend sends "APPROVE", "REJECT: <feedback>", or "CANCEL".
            let d = decision.trim();
            if d.eq_ignore_ascii_case("approve") || d.to_ascii_uppercase().starts_with("APPROVE") {
                let _ = ctx.db.update_agent_plan_status(&id, "applied");
                ctx.session_state.mark_plan_approved(&ctx.session_id);
                "The user APPROVED the plan. Proceed to execute it now.".into()
            } else if d.to_ascii_uppercase().starts_with("CANCEL: SUPERSEDED") {
                let _ = ctx.db.update_agent_plan_status(&id, "cancelled");
                "This plan was superseded by a newer presentation. Continue with the latest plan."
                    .into()
            } else if d.eq_ignore_ascii_case("cancel") || d.to_ascii_uppercase().starts_with("CANCEL") {
                let _ = ctx.db.update_agent_plan_status(&id, "cancelled");
                "The user cancelled the plan. Stop and ask what they want instead.".into()
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
            // Only a still-presented plan becomes "cancelled" on timeout; an
            // already-archived/applied plan keeps its status.
            let _ = ctx.db.update_agent_plan_status_if(&id, "cancelled", "presented");
            "error: the user did not respond to the plan in time".into()
        }
    }
}

// ---------------------------------------------------------------------------
// Goal-driven autonomous mode (/goal) tool handlers. The goal id is embedded in
// the session id ("goal:<id>"), mirroring how cron jobs use "cron:<id>".
// ---------------------------------------------------------------------------

fn latest_intake_goal_id(ctx: &ToolContext) -> Option<String> {
    ctx.db
        .list_goals()
        .ok()?
        .into_iter()
        .rev()
        .find(|g| g.status == "intake")
        .map(|g| g.id)
}

fn goal_session_mut(
    ctx: &ToolContext,
) -> Result<(String, crate::storage::models::GoalSession), String> {
    let goal_id = crate::ai::goal::goal_id_from_session(&ctx.session_id)
        .or_else(|| ctx.goal_id.clone().filter(|s| !s.is_empty()))
        .or_else(|| latest_intake_goal_id(ctx))
        .ok_or_else(|| "no active goal — start one with /goal <objective>".to_string())?;
    let goal = ctx
        .db
        .get_goal(&goal_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "goal session not found".to_string())?;
    Ok((goal_id, goal))
}

fn emit_goal_event(app: &tauri::AppHandle, goal_id: &str, event: StreamEvent) {
    let _ = app.emit(&crate::ai::goal::goal_event(goal_id), event);
}

async fn goal_propose_spec(ctx: &ToolContext, args: &Value) -> String {
    let (goal_id, mut goal) = match goal_session_mut(ctx) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    if goal.status != "intake" {
        return format!("error: goal is in '{}' status, not intake", goal.status);
    }
    let spec = crate::storage::models::GoalSpec {
        objective: args.get("objective").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        success_criteria: args
            .get("success_criteria")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        check_method: args.get("check_method").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        check_tooling: args
            .get("check_tooling")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        hard_constraints: args
            .get("hard_constraints")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        max_cycles: args.get("max_cycles").and_then(|v| v.as_i64()),
        vps_targets: crate::ai::goal::parse_spec(&goal)
            .map(|s| s.vps_targets)
            .unwrap_or_default(),
    };
    goal.spec_json = serde_json::to_string(&spec).unwrap_or_default();
    if let Err(e) = ctx.db.update_goal(&goal) {
        return format!("error: {e}");
    }
    emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("spec_proposed".into()));
    "Goal spec proposed. The user must review and click 'Lock goal & start' to activate it.".into()
}

fn goal_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn push_task_history(
    task: &mut crate::storage::models::GoalTask,
    action: &str,
    note: Option<String>,
) {
    let at = goal_now();
    task.history.push(crate::storage::models::GoalTaskEvent {
        at: at.clone(),
        action: action.to_string(),
        column: Some(task.column.clone()),
        note,
    });
    task.updated_at = Some(at);
}

async fn goal_add_task(ctx: &ToolContext, args: &Value) -> String {
    let (goal_id, mut goal) = match goal_session_mut(ctx) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let mut tasks = crate::ai::goal::parse_kanban(&goal);
    let parent_id = args
        .get("parent_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    if let Some(ref pid) = parent_id {
        if !tasks.iter().any(|t| t.id == *pid) {
            return format!("error: parent task '{pid}' not found");
        }
    }
    let now = goal_now();
    let column = args
        .get("column")
        .and_then(|v| v.as_str())
        .unwrap_or("backlog")
        .to_string();
    let detail = args.get("detail").and_then(|v| v.as_str()).map(String::from);
    let mut task = crate::storage::models::GoalTask {
        id: Uuid::new_v4().to_string(),
        column: column.clone(),
        title: args.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        detail: detail.clone(),
        kind: args.get("kind").and_then(|v| v.as_str()).unwrap_or("task").to_string(),
        files: args
            .get("files")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        result: None,
        error: None,
        parent_id,
        history: Vec::new(),
        created_at: Some(now.clone()),
        updated_at: Some(now),
    };
    push_task_history(&mut task, "created", detail);
    let id = task.id.clone();
    tasks.push(task);
    crate::ai::goal::set_kanban(&mut goal, tasks);
    if let Err(e) = ctx.db.update_goal(&goal) {
        return format!("error: {e}");
    }
    emit_goal_event(
        &ctx.app,
        &goal_id,
        StreamEvent::ToolResult { id: id.clone(), output: "task added".into() },
    );
    id
}

async fn goal_update_task(ctx: &ToolContext, args: &Value) -> String {
    let (goal_id, mut goal) = match goal_session_mut(ctx) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
    if task_id.is_empty() {
        return "error: task_id required".into();
    }
    let mut tasks = crate::ai::goal::parse_kanban(&goal);
    let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) else {
        return format!("error: task '{task_id}' not found");
    };
    let mut action = "updated";
    if let Some(col) = args.get("column").and_then(|v| v.as_str()) {
        if col != task.column {
            action = "moved";
        }
        task.column = col.to_string();
    }
    if let Some(title) = args.get("title").and_then(|v| v.as_str()) {
        task.title = title.to_string();
    }
    if let Some(d) = args.get("detail").and_then(|v| v.as_str()) {
        task.detail = Some(d.to_string());
        if action == "updated" {
            action = "detail";
        }
    }
    if let Some(k) = args.get("kind").and_then(|v| v.as_str()) {
        task.kind = k.to_string();
    }
    if let Some(r) = args.get("result").and_then(|v| v.as_str()) {
        task.result = Some(r.to_string());
        action = "result";
    }
    if let Some(e) = args.get("error").and_then(|v| v.as_str()) {
        task.error = Some(e.to_string());
        action = "error";
    }
    if let Some(f) = args.get("files").and_then(|v| v.as_array()) {
        task.files = f.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        if action == "updated" {
            action = "files";
        }
    }
    let note = args
        .get("note")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            if action == "result" {
                task.result.clone()
            } else if action == "error" {
                task.error.clone()
            } else {
                None
            }
        });
    if args.get("note").and_then(|v| v.as_str()).is_some() && action == "updated" {
        action = "note";
    }
    push_task_history(task, action, note);
    crate::ai::goal::set_kanban(&mut goal, tasks);
    if let Err(e) = ctx.db.update_goal(&goal) {
        return format!("error: {e}");
    }
    emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("task_updated".into()));
    "ok".into()
}

async fn goal_record_constraint(ctx: &ToolContext, args: &Value) -> String {
    let (goal_id, mut goal) = match goal_session_mut(ctx) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let value = args.get("value").and_then(|v| v.as_str()).unwrap_or("");
    let evidence = args.get("evidence").and_then(|v| v.as_str()).unwrap_or("");
    let confidence = args.get("confidence").and_then(|v| v.as_str()).unwrap_or("observed");
    if key.is_empty() || value.is_empty() {
        return "error: key and value required".into();
    }
    // Constraint memory: {"learned":[{key,value,evidence,confidence}], "history":[]}.
    let mut mem: serde_json::Value =
        serde_json::from_str(&goal.memory_json).unwrap_or_else(|_| serde_json::json!({}));
    if mem.get("learned").and_then(|l| l.as_array()).is_none() {
        mem["learned"] = serde_json::json!([]);
    }
    let learned = mem["learned"].as_array_mut().unwrap();
    // Replace an existing entry with the same key.
    learned.retain(|e| e.get("key").and_then(|k| k.as_str()) != Some(key));
    learned.push(serde_json::json!({
        "key": key,
        "value": value,
        "evidence": evidence,
        "confidence": confidence,
    }));
    goal.memory_json = serde_json::to_string(&mem).unwrap_or_default();
    if let Err(e) = ctx.db.update_goal(&goal) {
        return format!("error: {e}");
    }
    emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("memory_updated".into()));
    "ok".into()
}

async fn goal_check_criteria(ctx: &ToolContext, args: &Value) -> String {
    let (goal_id, mut goal) = match goal_session_mut(ctx) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let verdict = args.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
    let evidence = args.get("evidence").and_then(|v| v.as_str()).unwrap_or("");
    match verdict {
        "met" => {
            goal.status = "done".to_string();
            goal.finished_at = Some(chrono::Utc::now().to_rfc3339());
            // The evidence was already being asked for and then thrown away. Kept, it
            // is the difference between "this task ended" and "here is what came of
            // it" — the only version of the record worth reading a week later.
            if !evidence.trim().is_empty() {
                goal.outcome = Some(evidence.trim().to_string());
            }
            if let Err(e) = ctx.db.update_goal(&goal) {
                return format!("error: {e}");
            }
            emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("done".into()));
            format!("Goal marked done. Evidence: {evidence}")
        }
        "not_yet" => {
            emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("active".into()));
            "Not yet met — continue working.".into()
        }
        "too_early_to_tell" => {
            let delay = args.get("delay_secs").and_then(|v| v.as_i64()).unwrap_or(0);
            if delay > 0 {
                goal.status = "waiting".to_string();
                goal.next_check_at = Some(
                    (chrono::Utc::now() + chrono::Duration::seconds(delay)).to_rfc3339(),
                );
                if let Err(e) = ctx.db.update_goal(&goal) {
                    return format!("error: {e}");
                }
                emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("waiting".into()));
                format!("Too early to tell — re-check in {delay}s. Evidence: {evidence}")
            } else {
                emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("active".into()));
                format!("Too early to tell — keep scanning (no delay set). Evidence: {evidence}")
            }
        }
        other => format!("error: unknown verdict '{other}' (use met|not_yet|too_early_to_tell)"),
    }
}

async fn goal_schedule_wait(ctx: &ToolContext, args: &Value) -> String {
    let (goal_id, mut goal) = match goal_session_mut(ctx) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let until = args.get("until").and_then(|v| v.as_str()).unwrap_or("");
    if chrono::DateTime::parse_from_rfc3339(until).is_err() {
        return format!("error: 'until' must be RFC3339, got '{until}'");
    }
    let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("");
    goal.status = "waiting".to_string();
    goal.next_check_at = Some(until.to_string());
    if let Err(e) = ctx.db.update_goal(&goal) {
        return format!("error: {e}");
    }
    emit_goal_event(&ctx.app, &goal_id, StreamEvent::Status("waiting".into()));
    format!("Goal paused until {until}. Reason: {reason}")
}

// ---------------------------------------------------------------------------
// Creative & Visual Tools: SVG Generation, Image Generation, Live Canvas Preview
// ---------------------------------------------------------------------------

async fn generate_svg_tool(ctx: &ToolContext, args: &Value) -> String {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("graphic");
    let svg_content = args.get("svg_content").and_then(|v| v.as_str()).unwrap_or("");
    let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let target_dir_str = args.get("target_dir").and_then(|v| v.as_str());

    if svg_content.trim().is_empty() {
        return "error: missing required 'svg_content'".to_string();
    }
    if !svg_content.contains("<svg") || !svg_content.contains("</svg>") {
        return "error: 'svg_content' must contain valid <svg ...>...</svg> markup".to_string();
    }

    let clean_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let filename = format!("{clean_name}.svg");

    let dest_dir = if let Some(td) = target_dir_str.filter(|s| !s.trim().is_empty()) {
        std::path::PathBuf::from(td)
    } else {
        ctx.home.0.join("artifacts").join("svg")
    };

    if let Err(e) = tokio::fs::create_dir_all(&dest_dir).await {
        return format!("error creating destination directory: {e}");
    }

    let file_path = dest_dir.join(&filename);
    if let Err(e) = tokio::fs::write(&file_path, svg_content.as_bytes()).await {
        return format!("error writing svg file: {e}");
    }

    let abs_path = file_path.to_string_lossy().to_string();
    let size_bytes = svg_content.len();
    let desc_str = if description.is_empty() { String::new() } else { format!(" · {description}") };

    let _ = ctx.app.emit("agent://artifact-created", json!({
        "name": filename,
        "path": abs_path,
        "kind": "svg",
        "size": size_bytes,
        "description": description,
    }));

    format!("Generated SVG graphic '{filename}' ({size_bytes} bytes){desc_str}\nPath: {abs_path}\n\n```svg\n{svg_content}\n```")
}

async fn generate_image_tool(ctx: &ToolContext, args: &Value) -> String {
    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    if prompt.trim().is_empty() {
        return "error: missing required 'prompt'".to_string();
    }
    let aspect_ratio = args.get("aspect_ratio").and_then(|v| v.as_str()).unwrap_or("1:1");
    let name = args.get("name").and_then(|v| v.as_str());

    let (width, height) = crate::ai::image_gen::dimensions_from_aspect(aspect_ratio);

    let dest_dir = ctx.home.0.join("artifacts").join("images");

    let openai_key: Option<String> = ctx
        .db
        .get_provider("openai")
        .ok()
        .flatten()
        .and_then(|p| crate::secrets::get_secret(&crate::secrets::provider_key(&p.id)).ok().flatten().map(|z| z.to_string()))
        .filter(|k| !k.is_empty());

    let (saved_path, provider_used) = if let Some(key) = &openai_key {
        match crate::ai::image_gen::generate_openai(key, prompt, width, height, &dest_dir, name).await {
            Ok(p) => (p, "OpenAI DALL-E 3"),
            Err(e) => {
                crate::diag(&format!("OpenAI image generation failed, falling back to Pollinations/Flux: {e}"));
                match crate::ai::image_gen::generate_pollinations(prompt, width, height, &dest_dir, name).await {
                    Ok(p) => (p, "Flux (Pollinations)"),
                    Err(e2) => return format!("error generating image: {e} | fallback error: {e2}"),
                }
            }
        }
    } else {
        match crate::ai::image_gen::generate_pollinations(prompt, width, height, &dest_dir, name).await {
            Ok(p) => (p, "Flux (Pollinations)"),
            Err(e) => return format!("error generating image: {e}"),
        }
    };

    let abs_path = saved_path.to_string_lossy().to_string();
    let filename = saved_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image.png".to_string());

    let _ = ctx.app.emit("agent://artifact-created", json!({
        "name": filename,
        "path": abs_path,
        "kind": "image",
        "prompt": prompt,
        "width": width,
        "height": height,
        "provider": provider_used,
    }));

    format!("Generated image via {provider_used} ({width}x{height})\nPath: {abs_path}\n\n![{prompt}](file:///{abs_path})")
}

fn canvas_open_preview(ctx: &ToolContext, args: &Value) -> String {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Live Preview");
    let html = args.get("html").and_then(|v| v.as_str()).unwrap_or("");
    let width = args.get("width").and_then(|v| v.as_i64()).unwrap_or(800) as i32;
    let height = args.get("height").and_then(|v| v.as_i64()).unwrap_or(600) as i32;

    if html.trim().is_empty() {
        return "error: missing required 'html'".to_string();
    }

    let node_id = format!("preview-{}", &Uuid::new_v4().to_string()[..8]);
    let _ = ctx.app.emit("canvas://open-preview", json!({
        "id": node_id,
        "title": title,
        "html": html,
        "width": width,
        "height": height,
    }));

    format!("Opened preview sandbox '{title}' (node_id: {node_id}) on the canvas.")
}

#[cfg(test)]
mod edit_request_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_single_edit_still_needs_no_array() {
        // A one-line fix should not have to be wrapped in a list; making `edits`
        // mandatory would tax the common case to serve the rare one.
        let got = requested_edits(&json!({"old_string": "a", "new_string": "b"})).unwrap();
        assert_eq!(got, vec![("a".into(), "b".into(), false)]);
    }

    #[test]
    fn a_batch_is_read_in_order_with_its_flags() {
        let got = requested_edits(&json!({"edits": [
            {"old_string": "a", "new_string": "b"},
            {"old_string": "c", "new_string": "d", "replace_all": true}
        ]}))
        .unwrap();
        assert_eq!(got.len(), 2);
        assert!(!got[0].2);
        assert!(got[1].2);
    }

    #[test]
    fn the_batch_wins_when_both_are_sent() {
        // A model that fills in both should get the one it clearly meant, not a silent
        // half-application of the other.
        let got = requested_edits(&json!({
            "old_string": "ignored", "new_string": "x",
            "edits": [{"old_string": "a", "new_string": "b"}]
        }))
        .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "a");
    }

    #[test]
    fn an_empty_or_malformed_request_says_which_part_is_wrong() {
        assert!(requested_edits(&json!({"path": "/x"})).unwrap_err().contains("old_string"));
        assert!(requested_edits(&json!({"edits": []})).unwrap_err().contains("empty"));
        let err = requested_edits(&json!({"edits": [{"new_string": "b"}]})).unwrap_err();
        assert!(err.contains("edit 1"), "{err}");
    }

    #[test]
    fn a_batch_over_one_file_is_a_single_rewrite() {
        // The whole reason it exists: over SSH each edit reads and writes the entire
        // file, so five calls is five of each and one call is one.
        let (out, replacements, edits) = edit_contents(
            "alpha\nbeta\ngamma\n",
            &json!({"edits": [
                {"old_string": "alpha", "new_string": "1"},
                {"old_string": "gamma", "new_string": "3"}
            ]}),
        )
        .unwrap();
        assert_eq!(out, "1\nbeta\n3\n");
        assert_eq!((replacements, edits), (2, 2));
    }
}

#[cfg(test)]
mod command_timeout_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_default_stays_two_minutes() {
        // Most commands are quick, and a long default turns a hung one into a long
        // silence. What changed is that it is no longer also the ceiling.
        assert_eq!(command_timeout(&json!({})), crate::ssh::command::COMMAND_TIMEOUT);
        assert_eq!(command_timeout(&json!({"command": "ls"})), crate::ssh::command::COMMAND_TIMEOUT);
    }

    #[test]
    fn a_caller_can_wait_longer_for_work_that_takes_longer() {
        // A build or a migration that needs five minutes is not a failure, and calling
        // it one taught the agent to split work that has to run in one go.
        assert_eq!(command_timeout(&json!({"timeout_secs": 600})).as_secs(), 600);
    }

    #[test]
    fn a_nonsense_deadline_falls_back_rather_than_failing_instantly() {
        // Zero would make every command time out immediately, which reads as the server
        // being broken.
        assert_eq!(command_timeout(&json!({"timeout_secs": 0})), crate::ssh::command::COMMAND_TIMEOUT);
        assert_eq!(command_timeout(&json!({"timeout_secs": "ten"})), crate::ssh::command::COMMAND_TIMEOUT);
    }

    #[test]
    fn the_ceiling_is_enforced_where_the_command_runs() {
        // An hour is past the point where waiting synchronously was the right choice;
        // the clamp lives with the runner so every caller gets it.
        assert_eq!(crate::ssh::command::MAX_COMMAND_TIMEOUT.as_secs(), 3600);
        assert_eq!(crate::local::MAX_LOCAL_COMMAND_TIMEOUT.as_secs(), 3600);
    }
}

#[cfg(test)]
mod tool_pairing_tests {
    use super::*;

    /// Every tool that acts on files should work the same on a server and on this PC.
    ///
    /// The two searches had drifted apart while nobody was comparing them — the local
    /// one lost its fallback, its overall cap and its context lines, and each fix landed
    /// in one copy. A missing twin is the same failure one step earlier: the agent finds
    /// a tool for a server, looks for the local one, and has to improvise.
    #[test]
    fn every_file_tool_exists_on_both_sides() {
        let defs = definitions(&crate::ai::AgentHome::new(std::env::temp_dir().join("xc-pair-test")));
        let names: std::collections::HashSet<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        for base in ["read_file", "write_file", "edit_file", "grep_search", "find_files", "list_dir"] {
            assert!(names.contains(base), "no server tool `{base}`");
            let local = format!("local_{base}");
            assert!(names.contains(local.as_str()), "no local twin `{local}`");
        }
    }
}

#[cfg(test)]
mod list_dir_tests {
    use super::*;

    fn run(cmd: &str) -> String {
        let out = std::process::Command::new("sh").arg("-c").arg(cmd).output().expect("shell runs");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xc-ls-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden"), "y").unwrap();
        dir
    }

    #[test]
    #[cfg(unix)]
    fn a_listing_includes_hidden_files_and_a_total() {
        // A dotfile is usually exactly what is being looked for — an .env, a .gitignore,
        // a .service override — so hiding them by default would hide the answer.
        let dir = fixture("basic");
        let out = run(&list_dir_command(dir.to_str().unwrap(), true, 100));
        assert!(out.contains("a.txt"), "{out}");
        assert!(out.contains(".hidden"), "dotfiles missing: {out}");
        assert!(out.contains("sub"), "{out}");
        assert!(out.contains("__XCONS_TOTAL__ 3"), "wrong total: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_missing_path_and_a_file_are_told_apart() {
        // "No such directory" and "that is a file" lead to completely different next
        // steps, and an empty listing implies neither.
        let dir = fixture("kinds");
        assert!(run(&list_dir_command("/definitely/not/here", true, 10)).contains("NOT_FOUND"));
        let file = dir.join("a.txt");
        assert!(run(&list_dir_command(file.to_str().unwrap(), true, 10)).contains("NOT_A_DIRECTORY"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn the_total_is_the_real_count_even_when_the_listing_is_capped() {
        // A capped listing that reported only what it showed would let an agent
        // conclude a directory has three files when it has three hundred.
        let dir = fixture("capped");
        for i in 0..30 {
            std::fs::write(dir.join(format!("f{i}.txt")), "x").unwrap();
        }
        let out = run(&list_dir_command(dir.to_str().unwrap(), true, 5));
        assert!(out.contains("__XCONS_TOTAL__ 33"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_hostile_path_is_listed_not_executed() {
        let out = run(&list_dir_command("/tmp'; touch /tmp/xc-ls-pwned; '", true, 5));
        assert!(!std::path::Path::new("/tmp/xc-ls-pwned").exists(), "command injection: {out}");
    }
}

#[cfg(test)]
mod read_request_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn one_file_still_needs_no_array() {
        assert_eq!(requested_paths(&json!({"path": "/etc/nginx.conf"})).unwrap(), vec!["/etc/nginx.conf"]);
    }

    #[test]
    fn several_files_come_back_in_order() {
        // The ordinary shape of finding something out: a config, what it includes, and
        // the unit that starts it. Three round trips over SSH became one.
        let got = requested_paths(&json!({"paths": ["/a.conf", "/b.conf", "/c.service"]})).unwrap();
        assert_eq!(got, vec!["/a.conf", "/b.conf", "/c.service"]);
    }

    #[test]
    fn blanks_are_dropped_rather_than_read_as_a_file() {
        // An empty string reaches `cat` as the current directory, which errors in a way
        // that looks like the file being missing.
        let got = requested_paths(&json!({"paths": ["/a", "", "  ", "/b"]})).unwrap();
        assert_eq!(got, vec!["/a", "/b"]);
    }

    #[test]
    fn nothing_to_read_says_so_instead_of_reading_nothing() {
        assert!(requested_paths(&json!({})).unwrap_err().contains("path"));
        assert!(requested_paths(&json!({"paths": []})).unwrap_err().contains("empty"));
        assert!(requested_paths(&json!({"paths": ["", " "]})).unwrap_err().contains("empty"));
    }

    #[test]
    fn the_array_wins_when_both_are_given() {
        let got = requested_paths(&json!({"path": "/ignored", "paths": ["/wanted"]})).unwrap();
        assert_eq!(got, vec!["/wanted"]);
    }
}

#[cfg(test)]
mod explore_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn asking_a_question_is_not_a_mutation() {
        // It has to stay available in plan mode: investigating is exactly what plan mode
        // is for, and withholding the tool that investigates best would defeat it.
        assert!(!tool_is_mutating("explore", &json!({"question": "where is the handler"})));
    }

    #[test]
    fn the_tool_is_declared_and_routed() {
        // A tool the model can see but the dispatcher does not know answers "unknown
        // tool" at run time, which stays invisible until somebody asks for it.
        let defs = definitions(&crate::ai::AgentHome::new(std::env::temp_dir().join("xc-defs-test")));
        assert!(defs.iter().any(|d| d.name == "explore"), "explore is not declared");
    }
}

#[cfg(test)]
mod grep_command_tests {
    use super::*;

    fn run(cmd: &str, dir: &std::path::Path) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .output()
            .expect("shell runs");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// A tree of its own per test.
    ///
    /// Keyed by the caller's name, not the process id: every test in a binary shares
    /// one pid, so they were all creating and deleting the *same* directory while
    /// running in parallel, and failing on each other's cleanup.
    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xc-grep-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // One file with many hits, one with a few: the shape that exposed the bug.
        std::fs::write(dir.join("big.txt"), "needle\n".repeat(200)).unwrap();
        std::fs::write(dir.join("small.txt"), "one\nneedle\nthree\n").unwrap();
        std::fs::write(dir.join("other.log"), "needle in a log\n").unwrap();
        dir
    }

    #[test]
    #[cfg(unix)]
    fn one_huge_file_cannot_crowd_out_every_other_result() {
        const FIXTURE: &str = "one_huge_file_cannot_crowd_out_every_other_result";
        // ripgrep's `-m` caps matches per *file*, not overall. With that as the only
        // limit, a pattern common across many files returned thousands of lines, the
        // result cap then truncated it to an arbitrary slice, and the agent saw a
        // fraction of the answer with no sign that it had.
        let dir = fixture(FIXTURE);
        let cmd = grep_command(&GrepArgs { pattern: "needle", path: dir.to_str().unwrap(), head: 10, ..GrepArgs::new("needle", dir.to_str().unwrap()) });
        let out = run(&cmd, &dir);
        assert!(out.lines().count() <= 10, "returned {} lines", out.lines().count());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn context_lines_show_what_the_match_is_doing() {
        const FIXTURE: &str = "context_lines_show_what_the_match_is_doing";
        // The whole point: seeing the surrounding lines is usually why a read_file call
        // followed a search at all.
        let dir = fixture(FIXTURE);
        let cmd = grep_command(&GrepArgs { pattern: "needle", path: dir.join("small.txt").to_str().unwrap(), head: 40, context: 1, ..GrepArgs::new("needle", dir.join("small.txt").to_str().unwrap()) });
        let out = run(&cmd, &dir);
        assert!(out.contains("one"), "missing the line before: {out}");
        assert!(out.contains("three"), "missing the line after: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn files_mode_answers_where_without_the_contents() {
        const FIXTURE: &str = "files_mode_answers_where_without_the_contents";
        let dir = fixture(FIXTURE);
        let cmd = grep_command(&GrepArgs { pattern: "needle", path: dir.to_str().unwrap(), head: 40, mode: GrepOutput::Files, ..GrepArgs::new("needle", dir.to_str().unwrap()) });
        let out = run(&cmd, &dir);
        assert!(out.contains("small.txt") && out.contains("big.txt"), "{out}");
        // Names, not 200 copies of the line.
        assert!(out.lines().count() <= 5, "should be file names only: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_glob_narrows_to_the_files_asked_for() {
        const FIXTURE: &str = "a_glob_narrows_to_the_files_asked_for";
        let dir = fixture(FIXTURE);
        let cmd = grep_command(&GrepArgs { pattern: "needle", path: dir.to_str().unwrap(), glob: Some("*.log"), head: 40, mode: GrepOutput::Files, ..GrepArgs::new("needle", dir.to_str().unwrap()) });
        let out = run(&cmd, &dir);
        assert!(out.contains("other.log"), "{out}");
        assert!(!out.contains("big.txt"), "glob was ignored: {out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Force the `grep` branch by making `command -v rg` fail.
    ///
    /// Both branches have to behave the same, and most servers have no ripgrep — so
    /// testing only the branch that happens to run on this machine tests the half that
    /// matters least.
    fn without_ripgrep(cmd: &str) -> String {
        format!("PATH=/nonexistent-for-test:$PATH; command() {{ return 1; }}; {cmd}")
    }

    #[test]
    #[cfg(unix)]
    fn the_plain_grep_fallback_behaves_the_same() {
        const FIXTURE: &str = "the_plain_grep_fallback_behaves_the_same";
        let dir = fixture(FIXTURE);
        let base = grep_command(&GrepArgs { pattern: "needle", path: dir.to_str().unwrap(), head: 10, ..GrepArgs::new("needle", dir.to_str().unwrap()) });
        let out = run(&without_ripgrep(&base), &dir);
        assert!(out.contains("needle"), "fallback found nothing: {out}");
        assert!(out.lines().count() <= 10, "fallback ignored the cap: {}", out.lines().count());

        // And its glob filter, which is a different flag from ripgrep's.
        let globbed = grep_command(&GrepArgs { pattern: "needle", path: dir.to_str().unwrap(), glob: Some("*.log"), head: 40, mode: GrepOutput::Files, ..GrepArgs::new("needle", dir.to_str().unwrap()) });
        let out = run(&without_ripgrep(&globbed), &dir);
        assert!(out.contains("other.log"), "{out}");
        assert!(!out.contains("big.txt"), "fallback ignored the glob: {out}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_pattern_with_shell_characters_is_searched_not_executed() {
        const FIXTURE: &str = "a_pattern_with_shell_characters_is_searched_not_executed";
        // The pattern comes from the model and reaches a shell.
        let dir = fixture(FIXTURE);
        let cmd = grep_command(&GrepArgs { pattern: "x'; touch /tmp/xc-pwned; '", path: dir.to_str().unwrap(), head: 5, ..GrepArgs::new("x'; touch /tmp/xc-pwned; '", dir.to_str().unwrap()) });
        let _ = run(&cmd, &dir);
        assert!(!std::path::Path::new("/tmp/xc-pwned").exists(), "command injection");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod write_verification_tests {
    use super::*;

    #[test]
    fn the_size_and_mtime_come_off_the_end_of_the_write() {
        let (mtime, size) = parse_stat_line("exit_code: 0\nxc-wrote 1735600000 4096\n");
        assert_eq!(mtime, "1735600000");
        assert_eq!(size, Some(4096));
    }

    #[test]
    fn chatter_before_the_stat_line_is_ignored() {
        // Profile scripts, sudo banners and motd all print before the command does.
        // Including a trailing line that is two plain numbers: an unmarked reverse
        // scan would have taken it and reported a made-up size as verified.
        let out = "exit_code: 0\nWelcome to Ubuntu 22.04\nxc-wrote 99 7\n1 2\n";
        assert_eq!(parse_stat_line(out), ("99".into(), Some(7)));
    }

    #[test]
    fn a_server_without_stat_reports_unknown_rather_than_a_wrong_size() {
        // BusyBox and some BSDs do not take `-c`. Reporting "wrote 0 bytes,
        // verified" there would be a lie told confidently, which is worse than
        // admitting the size could not be read back.
        let (mtime, size) = parse_stat_line("exit_code: 0\nstat: invalid option -- 'c'\n");
        assert!(mtime.is_empty());
        assert_eq!(size, None);
    }

    #[test]
    fn an_append_adds_to_the_file_rather_than_replacing_it() {
        // Verified against a real shell, because the whole change is one character
        // of redirection and reading `>>` as correct is not the same as it being so.
        let dir = std::env::temp_dir().join(format!("xc-append-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("out.txt");
        for (mode, text) in [(WriteMode::Overwrite, "one\n"), (WriteMode::Append, "two\n")] {
            let redirect = if mode == WriteMode::Append { ">>" } else { ">" };
            let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
            let cmd = format!(
                "printf %s {} | base64 -d {redirect} {}",
                shell_quote(&b64),
                shell_quote(f.to_str().unwrap())
            );
            let ok = std::process::Command::new("sh")
                .args(["-c", &cmd])
                .status()
                .expect("shell runs")
                .success();
            assert!(ok, "write failed for {mode:?}");
        }
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "one\ntwo\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_mode_defaults_to_overwrite_and_nothing_else_turns_it_on() {
        assert_eq!(WriteMode::parse(&json!({})), WriteMode::Overwrite);
        assert_eq!(WriteMode::parse(&json!({"mode": "append"})), WriteMode::Append);
        // A typo must not silently append to a file the agent meant to replace,
        // leaving it with two copies of everything.
        assert_eq!(WriteMode::parse(&json!({"mode": "Append"})), WriteMode::Overwrite);
        assert_eq!(WriteMode::parse(&json!({"mode": "add"})), WriteMode::Overwrite);
    }

    #[test]
    fn both_write_tools_offer_the_same_modes() {
        let defs = definitions(&crate::ai::AgentHome::new(
            std::env::temp_dir().join("xc-mode-defs-test"),
        ));
        for name in ["write_file", "local_write_file"] {
            let def = defs.iter().find(|d| d.name == name).expect("declared");
            assert!(
                def.parameters["properties"].get("mode").is_some(),
                "{name} cannot append, so content too long for one call cannot be written at all"
            );
        }
    }

    #[test]
    fn a_checksum_is_read_off_an_upload_or_not_read_at_all() {
        assert_eq!(
            remote_sha256(
                "xc-wrote 1735600000 12\n\
                 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08  /tmp/f"
            )
            .as_deref(),
            Some("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")
        );
        // A box without sha256sum must yield nothing rather than a guess: a wrong
        // hash here condemns a perfectly good upload as corrupt.
        assert_eq!(remote_sha256("sh: sha256sum: not found"), None);
        assert_eq!(remote_sha256("xc-wrote 1735600000 12"), None);
        // BSD prints a different shape entirely. Not understood is not guessed at.
        assert_eq!(remote_sha256("SHA256 (f) = 9f86d081"), None);
    }

    #[test]
    fn a_download_is_refused_before_it_is_paid_for() {
        // The limit used to be checked after the file had crossed the wire as
        // base64 and been decoded in memory, so refusing a 2GB file cost 2.7GB of
        // RAM first. The guard has to be on the server, in the command.
        let cmd = download_command("/tmp/big.bin");
        assert!(
            cmd.find("wc -c").unwrap() < cmd.find("base64").unwrap(),
            "the size is still being learned after the transfer: {cmd}"
        );
        assert!(cmd.contains("xc-too-big"), "nothing tells the caller it was refused");
        // A path from the model is a path from whatever it has read — a filename on
        // a server, a web page, another tool's output. Assert on what the shell
        // does with it, not on how it looks: the escaped form legitimately contains
        // the dangerous text, and only running it says whether it stays inert.
        let dir = std::env::temp_dir().join(format!("xc-dl-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let canary = dir.join("canary");
        std::fs::write(&canary, "still here").unwrap();
        let nasty = format!("{}/x'; rm -f {} ; echo '", dir.display(), canary.display());
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(download_command(&nasty))
            .output()
            .expect("shell runs");
        assert!(
            canary.exists(),
            "the path broke out of its quotes and ran: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_timeout_the_model_asks_for_is_capped_at_the_advertised_hour() {
        // The tool description promises max 3600. Without the clamp a typo'd
        // 360000 parks the turn on a dead SSH read for four days.
        assert_eq!(
            command_timeout(&serde_json::json!({"timeout_secs": 360_000})),
            std::time::Duration::from_secs(3600)
        );
        assert_eq!(
            command_timeout(&serde_json::json!({"timeout_secs": 600})),
            std::time::Duration::from_secs(600)
        );
        // Nonsense falls back to the default rather than to zero, which would
        // time out every command instantly.
        assert_eq!(
            command_timeout(&serde_json::json!({"timeout_secs": 0})),
            crate::ssh::command::COMMAND_TIMEOUT
        );
    }

    #[test]
    fn every_target_of_run_command_all_honours_the_timeout() {
        let defs = definitions(&crate::ai::AgentHome::new(
            std::env::temp_dir().join("xc-all-defs-test"),
        ));
        let def = defs
            .iter()
            .find(|d| d.name == "run_command_all")
            .expect("run_command_all is declared");
        assert!(
            def.parameters["properties"].get("timeout_secs").is_some(),
            "run_command_all could not be given more than the default 120s, so the one \
             tool meant for every server was the one that could not wait for a build"
        );
    }

    #[test]
    fn reading_a_terminal_can_ask_for_more_than_the_default_screen() {
        let defs = definitions(&crate::ai::AgentHome::new(
            std::env::temp_dir().join("xc-term-defs-test"),
        ));
        let def = defs
            .iter()
            .find(|d| d.name == "terminal_capture")
            .expect("terminal_capture is declared");
        let props = &def.parameters["properties"];
        assert!(props.get("max_chars").is_some(), "the 1800-char tail was not liftable");
        assert!(
            props.get("terminal_id").is_some(),
            "with several terminals on one server there was no way to say which to read"
        );
    }
}

#[cfg(test)]
mod local_freshness_tests {
    use crate::ai::file_state::{self, LOCAL};

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xc-fresh-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_local_file_changed_under_the_agent_is_not_overwritten() {
        // The machine running xConsole is the one with the git checkouts on it, and
        // the user has agents pulling in the background. Read, someone else writes,
        // write back: without the stamp that is a silent loss of their commit.
        let dir = fixture("clobber");
        let path = dir.join("app.rs");
        std::fs::write(&path, "original\n").unwrap();
        let p = path.to_str().unwrap();

        file_state::note_read("s1", LOCAL, p, &file_state::local_mtime(p));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "somebody else's work\n").unwrap();

        let err = file_state::check_write("s1", LOCAL, p, &file_state::local_mtime(p))
            .expect_err("the stale write was allowed through");
        assert!(err.contains("changed on disk"), "{err}");
        assert!(
            err.contains("local_read_file"),
            "a local path was told to re-read with the server tool: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_writes_in_a_row_to_a_local_file_are_allowed() {
        let dir = fixture("twice");
        let path = dir.join("notes.md");
        std::fs::write(&path, "one\n").unwrap();
        let p = path.to_str().unwrap();

        file_state::note_read("s2", LOCAL, p, &file_state::local_mtime(p));
        std::fs::write(&path, "two\n").unwrap();
        file_state::note_write("s2", LOCAL, p, &file_state::local_mtime(p));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "three\n").unwrap();
        file_state::note_write("s2", LOCAL, p, &file_state::local_mtime(p));

        assert!(
            file_state::check_write("s2", LOCAL, p, &file_state::local_mtime(p)).is_ok(),
            "the agent's own consecutive writes were reported as another process"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_does_not_exist_yet_is_never_a_clobber() {
        let dir = fixture("fresh");
        let p = dir.join("new.txt");
        assert!(file_state::local_mtime(p.to_str().unwrap()).is_empty());
        assert!(file_state::check_write("s3", LOCAL, p.to_str().unwrap(), "").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_and_remote_paths_do_not_share_a_stamp() {
        // /etc/nginx.conf exists on this PC and on every server. One key each, or
        // reading it here would authorise a write there.
        file_state::note_read("s4", LOCAL, "/etc/nginx.conf", "100");
        assert!(
            file_state::check_write("s4", "vps-uuid", "/etc/nginx.conf", "999").is_ok(),
            "a read of the local copy was accepted as a read of the server's"
        );
    }
}

#[cfg(test)]
mod grep_paging_tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xc-page-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(cmd: &str) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .expect("shell runs");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    fn a_truncated_search_can_be_continued_instead_of_narrowed() {
        // Without offset the only way past a cut-off result was to narrow the
        // pattern — guessing at the shape of matches you have not seen.
        let dir = fixture("offset");
        let body: String = (0..100).map(|i| format!("needle {i}\n")).collect();
        std::fs::write(dir.join("a.txt"), body).unwrap();
        let path = dir.to_str().unwrap();

        let first = run(&grep_command(&GrepArgs {
            head: 10,
            ..GrepArgs::new("needle", path)
        }));
        let second = run(&grep_command(&GrepArgs {
            head: 10,
            offset: 10,
            ..GrepArgs::new("needle", path)
        }));
        assert_eq!(first.lines().count(), 10, "{first}");
        assert_eq!(second.lines().count(), 10, "{second}");
        assert!(first.contains("needle 0"), "{first}");
        assert!(!second.contains("needle 0"), "the second page started over: {second}");
        assert!(second.contains("needle 10"), "the second page skipped matches: {second}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_note_names_the_offset_that_continues_the_search() {
        let body: String = (0..10).map(|i| format!("hit {i}\n")).collect();
        let note = grep_note(&body, 10, 0);
        assert!(note.contains("offset: 10"), "{note}");
        let later = grep_note(&body, 10, 40);
        assert!(later.contains("offset: 50"), "the offset did not accumulate: {later}");
        // Nothing to say when the result was complete.
        assert!(grep_note("one\ntwo\n", 10, 0).is_empty());
    }

    #[test]
    fn a_pattern_can_span_lines_where_ripgrep_exists() {
        let dir = fixture("multiline");
        std::fs::write(dir.join("a.rs"), "fn open(\n    path: &str,\n) {}\n").unwrap();
        let path = dir.to_str().unwrap();
        let cmd = grep_command(&GrepArgs {
            multiline: true,
            ..GrepArgs::new(r"fn open\(\s*path", path)
        });
        let out = run(&cmd);
        if std::process::Command::new("sh")
            .args(["-c", "command -v rg"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            assert!(out.contains("fn open"), "multiline search found nothing: {out}");
        } else {
            // No rg: it must say so rather than quietly answering "no matches",
            // which would read as "this pattern is not in the code".
            let full = run(&format!("{cmd} 2>&1"));
            assert!(full.contains("needs ripgrep"), "silently wrong without rg: {full}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_is_big_enough_for_an_ordinary_repository_search() {
        // 40 lines, hard-capped at 200, meant a normal search across a project was
        // truncated most times it ran.
        assert!(DEFAULT_GREP_LINES >= 250);
        assert!(MAX_GREP_LINES >= 2000);
        assert!(DEFAULT_FIND_RESULTS >= 200);
    }
}

#[cfg(test)]
mod ollama_tool_list_tests {
    use super::*;

    fn declared() -> Vec<String> {
        definitions(&crate::ai::AgentHome::new(
            std::env::temp_dir().join("xc-ollama-defs-test"),
        ))
        .into_iter()
        .map(|d| d.name)
        .collect()
    }

    #[test]
    fn every_name_on_the_lists_is_a_tool_that_exists() {
        // The lists are names in a const, so a rename or a typo does not fail the
        // build — it just quietly drops the tool for every local model.
        let have = declared();
        for name in OLLAMA_VPS_TOOLS.iter().chain(OLLAMA_LOCAL_TOOLS.iter()) {
            assert!(
                have.iter().any(|d| d == name),
                "{name} is offered to Ollama but no such tool is declared"
            );
        }
    }

    /// Tools that make no sense without their follow-up.
    ///
    /// Handing a model the tool that *starts* something without the tools that
    /// check on it or stop it is worse than not offering it at all: the work runs
    /// and the model has no way to find out how it went.
    const MUST_TRAVEL_TOGETHER: &[(&str, &[&str])] = &[
        ("run_command", &["job_status", "job_list", "job_kill"]),
        ("local_run_command", &["job_status", "job_list", "job_kill"]),
        ("write_file", &["read_file", "edit_file"]),
        ("local_write_file", &["local_read_file", "local_edit_file"]),
        ("terminal_send", &["terminal_capture"]),
        ("upload_file", &["download_file"]),
    ];

    fn check_pairs(list: &[&str], which: &str) {
        for (starter, followups) in MUST_TRAVEL_TOGETHER {
            if !list.contains(starter) {
                continue;
            }
            for f in *followups {
                assert!(
                    list.contains(f),
                    "{which} offers {starter} but not {f} — the model can start work \
                     it has no way to follow up on"
                );
            }
        }
    }

    #[test]
    fn a_local_model_can_follow_up_on_anything_it_can_start() {
        check_pairs(OLLAMA_VPS_TOOLS, "OLLAMA_VPS_TOOLS");
        check_pairs(OLLAMA_LOCAL_TOOLS, "OLLAMA_LOCAL_TOOLS");
    }

    #[test]
    fn looking_around_is_possible_on_both_sides() {
        // Reading a file you cannot find is guesswork. Whichever side of the
        // machine a list covers, it needs the tools that locate things there.
        for (list, which, needed) in [
            (
                OLLAMA_VPS_TOOLS,
                "OLLAMA_VPS_TOOLS",
                ["list_dir", "find_files", "grep_search"],
            ),
            (
                OLLAMA_LOCAL_TOOLS,
                "OLLAMA_LOCAL_TOOLS",
                ["local_list_dir", "local_find_files", "local_grep_search"],
            ),
        ] {
            for n in needed {
                assert!(list.contains(&n), "{which} cannot {n}");
            }
        }
    }
}
