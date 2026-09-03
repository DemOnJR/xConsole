//! Hand one job to an agent CLI running on a server.
//!
//! xConsole can already *be* Claude Code: point a provider row at a VPS and the whole
//! turn runs there ([`crate::ai::providers::cli::CliProvider::chat_remote`]). That is an
//! all-or-nothing choice made in settings — the agent driving the session either is the
//! remote CLI for every message or is never it.
//!
//! This is the other shape, and the one an agent asks for: "I am working on three
//! servers; go and do this one long job on web-1 with a coding agent that lives there,
//! and tell me what happened." A CLI on the box has the repository checked out, the
//! toolchain installed and a warm filesystem cache; driving the same work through
//! `run_command` means a round trip per file and a context window full of `cat` output.
//!
//! Three tools, in the order they are normally needed:
//!
//! * [`agent_cli_status`] — what is installed on that box and whether it can run
//!   unattended. Read-only, so it is safe to call before deciding anything.
//! * [`agent_cli_provision`] — create the unprivileged account the CLI runs as, and
//!   optionally install the CLI. Signing *in* is a browser or device flow that a person
//!   has to complete once; this says so rather than pretending otherwise.
//! * [`agent_run_cli`] — run one job.
//!
//! ## Why not root
//!
//! Nothing in xConsole used to know that an agent CLI should not run as root, and for
//! these hosts the SSH login *is* root. An agent with a filesystem tool, no supervision
//! and root is the difference between a bad edit and an unrecoverable one. So the run
//! goes through `sudo -n -u <agent user>` ([`crate::ssh::agent_exec::wrap_for_user`]),
//! and the account is created deliberately by [`agent_cli_provision`] rather than
//! assumed by a config default that would turn every run into `sudo: unknown user`.
//!
//! ## Credentials
//!
//! Never in a tool argument and never in the database. The CLI's own credentials live on
//! the box, under the agent user's home, put there by a human running the sign-in flow
//! once. The only secret this module handles is the one-run MCP bearer token, and that
//! goes to the far side in a mode-600 file with a cleanup trap, not in argv where `ps`
//! shows it to every account on the machine.

// Unreachable until the three registration lines land in `ai/tools.rs` (definitions,
// dispatch, tool_is_mutating) — that file is owned by another change in this wave, so the
// module is complete and wired to nothing. Remove this the moment it is registered:
// after that, a dead function here is a real one.


use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::ai::provider::{emit, ChatResponse, EventSink, StreamEvent, ToolDef};
use crate::ai::tools::ToolContext;
use crate::ssh::remote_ops::shell_quote;

/// Which agent CLIs may be driven remotely, and how each one is invoked.
///
/// Kept to the two whose non-interactive flags were read off `--help` on a machine that
/// has them installed. A guessed flag is a run that fails after five minutes with a
/// usage message, which is worse than saying the brain is not supported.
const BRAINS: &[&str] = &["claude_code", "grok_cli"];

/// Where the agent user for a box is remembered.
///
/// Per server, not per provider row: the account exists on the machine, and every tool
/// call that names that machine should use it — including one made by a session whose
/// own provider is an HTTP API on the other side of the world.
fn agent_user_key(vps_id: &str) -> String {
    format!("cli_agent_user:{vps_id}")
}

/// The account the CLI runs as on `vps_id`, when one has been provisioned.
fn agent_user(db: &crate::storage::Db, vps_id: &str) -> Option<String> {
    db.get_setting(&agent_user_key(vps_id))
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "agent_run_cli".into(),
            description: "Hand one self-contained job to a coding-agent CLI installed on a \
server, and get back what it did. The CLI runs there with the repository already checked \
out and the toolchain already installed, so it reads and edits files locally instead of \
paying an SSH round trip per file.\n\
USE IT FOR: a job you can state in a paragraph and check afterwards — \"upgrade the \
Django version in /srv/app and make the test suite pass\", \"work out why the nightly \
cron has been failing since Tuesday and fix it\". Anything that would otherwise be twenty \
run_command calls whose output you do not need to read.\n\
DO NOT USE IT FOR: something you can do in one or two commands (use run_command — this \
costs a whole extra model's context to start), anything you cannot describe precisely \
enough to check the answer, or a job on a box where agent_cli_status says the CLI is not \
signed in. It is not a way to skip thinking about the task: a vague brief produces \
confident nonsense on someone else's server.\n\
COST: a full agent session on the remote CLI's own subscription or key, billed there, \
not through this session's provider. Minutes, not seconds. Nothing is streamed back \
except progress lines and the final answer, so the work is only as reviewable as the \
report it writes — say in the task what evidence you want (\"end with the exact test \
command you ran and its output\").\n\
SAFETY: it runs as an unprivileged agent account, never root, and only after \
agent_cli_provision has created that account. It can edit files and run commands there \
on its own; treat starting one like handing someone else the keyboard."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string", "description": "Exact target UUID from list_vps_targets."},
                    "task": {
                        "type": "string",
                        "description": "The whole brief, in plain prose. The remote agent sees this and nothing else — not this conversation, not the user's request, not what you already tried. Include the paths, the definition of done, and how it should verify itself."
                    },
                    "cwd": {"type": "string", "description": "Absolute directory to run in — normally the repository root. Defaults to the agent account's home, which is almost never what you want."},
                    "brain": {
                        "type": "string",
                        "enum": ["claude_code", "grok_cli"],
                        "description": "Which CLI to drive (default claude_code). Only these two have verified non-interactive flags; asking for another is refused rather than guessed at."
                    },
                    "model": {"type": "string", "description": "Model id for that CLI. Omit to use whatever it is configured with there."},
                    "timeout_secs": {"type": "integer", "description": "Give up after this long (default 900, min 60, max 7200). The run is killed, not detached: pick a bound you are willing to wait for, and split the job if it needs more."},
                    "resume": {"type": "boolean", "description": "Continue the last session this tool ran on that server and directory, instead of starting cold. Use it for a follow-up (\"now do the same for the staging config\"); do not use it to retry a job that failed, because it will resume into the confusion that caused the failure."}
                },
                "required": ["vps_id", "task"]
            }),
        },
        ToolDef {
            name: "agent_cli_status".into(),
            description: "What agent CLIs are installed on a server, which account they would \
run as, and whether that account can run unattended. Read-only and cheap: it runs `--version` \
and a couple of `test -e` checks, changes nothing, and is the call to make BEFORE \
agent_run_cli rather than after it fails.\n\
Reports, per CLI: whether the binary is on the agent account's PATH, its version, and \
whether credentials exist under that account's home. \"Installed but not signed in\" is \
the common state and the one worth reporting to the user, because only a human can fix \
it — see agent_cli_provision."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string", "description": "Exact target UUID from list_vps_targets."}
                },
                "required": ["vps_id"]
            }),
        },
        ToolDef {
            name: "agent_cli_provision".into(),
            description: "Create the unprivileged account an agent CLI runs as on a server, and \
optionally install the CLI into it. Run this once per server, then agent_cli_status to \
confirm, then agent_run_cli.\n\
It creates the account if missing, gives it a home and a private config directory, and \
checks that this login can switch to it without a password. It is idempotent — running it \
twice on a provisioned box reports the state and changes nothing.\n\
WHAT IT CANNOT DO: sign the CLI in. Every one of these CLIs authenticates through a \
browser or a device code, which needs a person. This tool prints the exact command for \
the user to run and stops there; do not claim a server is ready until agent_cli_status \
says the credentials exist. Never ask the user for a token to pass in here — no \
credential belongs in a tool argument.\n\
It changes system state (creates a user, may install software), so it needs approval \
unless safety is set to full."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "vps_id": {"type": "string", "description": "Exact target UUID from list_vps_targets."},
                    "user": {"type": "string", "description": "Account name to create and remember for this server (default xconsole-agent). Lowercase letters, digits, dash and underscore only."},
                    "install": {
                        "type": "string",
                        "enum": ["none", "claude_code", "grok_cli"],
                        "description": "Also install this CLI into the account (default none). Installing downloads and runs the vendor's own installer as the agent user; skip it and install by hand if that is not acceptable on this box."
                    }
                },
                "required": ["vps_id"]
            }),
        },
    ]
}

pub fn is_cli_agent_tool(name: &str) -> bool {
    matches!(
        name,
        "agent_run_cli" | "agent_cli_status" | "agent_cli_provision"
    )
}

/// Asking what is installed changes nothing. Starting an agent on a server, and creating
/// an account to start it under, plainly do — so plan mode withholds both.
pub fn tool_is_mutating(name: &str) -> bool {
    name != "agent_cli_status"
}

pub async fn dispatch(ctx: &ToolContext, name: &str, args: &Value, sink: &EventSink) -> String {
    match name {
        "agent_cli_status" => status(ctx, args).await,
        "agent_cli_provision" => provision(ctx, args, sink).await,
        "agent_run_cli" => run_cli(ctx, args, sink).await,
        other => format!("error: unknown CLI-agent tool {other}"),
    }
}

// ---- shared helpers ---------------------------------------------------------------

fn vps_of(ctx: &ToolContext, args: &Value) -> Result<String, String> {
    let asked = args
        .get("vps_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if !asked.is_empty() {
        return Ok(asked);
    }
    match ctx.targets.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err("no server is selected for this session — pass vps_id".into()),
        many => Err(format!(
            "{} servers are selected, so vps_id is required (use list_vps_targets for the exact ids)",
            many.len()
        )),
    }
}

/// One command on the box, as `as_user` when given, with a wall-clock bound.
///
/// [`crate::ssh::agent_exec::run_agent`] streams without a timeout because an agent turn
/// takes as long as it takes. Every call here has a bound, so the timeout lives here and
/// the cancel flag is what stops the stream when it fires.
async fn run_there(
    db: &crate::storage::Db,
    vps_id: &str,
    as_user: Option<&str>,
    script: &str,
    timeout: Duration,
    mut on_line: impl FnMut(String),
) -> Result<(i32, String, String), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let lines = Arc::new(std::sync::Mutex::new(String::new()));
    let collector = lines.clone();
    let script = script.to_string();

    let fut = crate::ssh::agent_exec::run_agent(
        db,
        vps_id,
        as_user,
        None,
        "",
        move |_| script.clone(),
        Some(cancel.clone()),
        move |line| {
            let mut buf = collector.lock().unwrap();
            buf.push_str(&line);
            buf.push('\n');
            on_line(line);
        },
    );

    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(run)) => {
            let stdout = lines.lock().unwrap().clone();
            Ok((run.exit_code, stdout, run.stderr))
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            Err(format!(
                "timed out after {}s waiting for {vps_id}",
                timeout.as_secs()
            ))
        }
    }
}

/// A valid POSIX account name. Rejected rather than sanitised: silently renaming the
/// account the user asked for produces a box whose agent user is not the one they think.
fn valid_user(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !name.starts_with('-')
}

/// Where each CLI keeps the credentials a sign-in writes, relative to `$HOME`.
///
/// Existence is the only claim made — that a file is there, not that the token in it is
/// still valid. "Signed in once" and "signed in now" are different facts and only the
/// CLI itself knows the second one.
fn credential_paths(brain: &str) -> &'static [&'static str] {
    match brain {
        "claude_code" => &[".claude/.credentials.json", ".config/claude/credentials.json"],
        "grok_cli" => &[".grok/auth.json", ".grok/credentials.json"],
        _ => &[],
    }
}

fn brain_bin(brain: &str) -> &'static str {
    crate::ai::providers::cli::remote_default_bin(brain)
}

/// The command a human runs, on that box, to sign the CLI in.
///
/// Both are device/browser flows on purpose: there is no headless equivalent to
/// automate, and pretending there is produces a tool that reports success and a server
/// that still cannot run anything.
fn login_hint(brain: &str, user: &str) -> String {
    match brain {
        "grok_cli" => format!("sudo -u {user} -H grok login --device-auth"),
        _ => format!("sudo -u {user} -H claude setup-token"),
    }
}

// ---- agent_cli_status --------------------------------------------------------------

async fn status(ctx: &ToolContext, args: &Value) -> String {
    let vps_id = match vps_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let user = agent_user(&ctx.db, &vps_id);

    // One round trip for the whole picture. Each answer is prefixed so a missing line is
    // distinguishable from an empty one — "we did not check" and "it is not there" are
    // opposite conclusions.
    let mut script = String::new();
    script.push_str("echo \"whoami=$(id -un)\"; echo \"home=$HOME\"; ");
    for brain in BRAINS {
        let bin = brain_bin(brain);
        script.push_str(&format!(
            "if command -v {bin} >/dev/null 2>&1; then \
               echo \"{brain}.path=$(command -v {bin})\"; \
               echo \"{brain}.version=$({bin} --version 2>&1 | head -n1)\"; \
             else echo \"{brain}.path=\"; fi; "
        ));
        for path in credential_paths(brain) {
            script.push_str(&format!(
                "[ -e \"$HOME/{path}\" ] && echo \"{brain}.credentials=$HOME/{path}\"; "
            ));
        }
    }
    script.push_str("echo done");

    let (_, out, err) = match run_there(
        &ctx.db,
        &vps_id,
        user.as_deref(),
        &script,
        Duration::from_secs(60),
        |_| {},
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };

    if user.is_some() && crate::ssh::agent_exec::is_sudo_denied(&err) {
        return format!(
            "error: this login cannot switch to '{}' on {vps_id} without a password \
             ({}). Fix sudoers on that server, or run agent_cli_provision again.",
            user.unwrap_or_default(),
            err.trim().lines().next().unwrap_or("sudo refused")
        );
    }

    let fields: std::collections::HashMap<&str, &str> = out
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();

    let mut report = String::new();
    report.push_str(&format!("server: {vps_id}\n"));
    match &user {
        Some(u) => report.push_str(&format!(
            "agent account: {u} (runs as {})\n",
            fields.get("whoami").copied().unwrap_or("?")
        )),
        None => report.push_str(&format!(
            "agent account: none provisioned — the CLI would run as {}, which is what \
             agent_cli_provision exists to avoid. Run it before agent_run_cli.\n",
            fields.get("whoami").copied().unwrap_or("the login user")
        )),
    }

    let mut any_ready = false;
    for brain in BRAINS {
        let path = fields.get(format!("{brain}.path").as_str()).copied().unwrap_or("");
        if path.is_empty() {
            report.push_str(&format!("{brain}: not installed\n"));
            continue;
        }
        let version = fields
            .get(format!("{brain}.version").as_str())
            .copied()
            .unwrap_or("unknown version");
        let creds = fields.get(format!("{brain}.credentials").as_str()).copied();
        match creds {
            Some(c) => {
                any_ready = true;
                report.push_str(&format!("{brain}: {path} ({version}) — credentials at {c}\n"));
            }
            None => report.push_str(&format!(
                "{brain}: {path} ({version}) — NOT signed in. A person has to run \
                 `{}` on that server once; it is a browser/device flow and cannot be \
                 done from here.\n",
                login_hint(brain, user.as_deref().unwrap_or("<agent-user>"))
            )),
        }
    }

    if !any_ready {
        report.push_str(
            "\nNothing on this server can run a job yet. agent_run_cli will fail until one \
             of the above is installed and signed in.\n",
        );
    }
    report
}

// ---- agent_cli_provision -----------------------------------------------------------

async fn provision(ctx: &ToolContext, args: &Value, sink: &EventSink) -> String {
    let vps_id = match vps_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let user = args
        .get("user")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("xconsole-agent")
        .to_string();
    if !valid_user(&user) {
        return format!(
            "error: '{user}' is not a usable account name — lowercase letters, digits, \
             dash and underscore, at most 32 characters."
        );
    }
    let install = args
        .get("install")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "none")
        .map(str::to_string);
    if let Some(b) = &install {
        if !BRAINS.contains(&b.as_str()) {
            return format!(
                "error: no verified installer for '{b}'. Supported: {}. Install it by \
                 hand and re-run agent_cli_status.",
                BRAINS.join(", ")
            );
        }
    }

    // Creating a system account and installing software is a change to the machine, so
    // it goes through the same gate as any other command the agent runs there.
    let summary = format!(
        "provision agent account '{user}' on {vps_id}{}",
        install
            .as_deref()
            .map(|b| format!(" and install {b}"))
            .unwrap_or_default()
    );
    if let Err(e) = crate::ai::safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &ctx.safety,
        &ctx.session_id,
        Some(&vps_id),
        &summary,
    )
    .await
    {
        return format!("error: {e}");
    }

    emit(Some(sink), StreamEvent::Status(format!("Provisioning {user} on {vps_id}…")));

    let q = shell_quote(&user);
    // Every step is a no-op when it has already been done, so this can be re-run after a
    // half-finished attempt without unpicking anything first.
    let mut script = format!(
        "set -e; \
         if id -u {q} >/dev/null 2>&1; then echo 'user.exists=1'; else \
           useradd --create-home --shell /bin/bash {q} && echo 'user.created=1'; fi; \
         install -d -m 700 -o {q} -g {q} \"$(getent passwd {q} | cut -d: -f6)/.claude\"; \
         install -d -m 700 -o {q} -g {q} \"$(getent passwd {q} | cut -d: -f6)/.grok\"; \
         echo \"home=$(getent passwd {q} | cut -d: -f6)\"; "
    );
    // Whether *this* login can become that user without a password is the one thing that
    // decides if unattended runs work at all, so it is checked here rather than
    // discovered later by a job that hangs.
    script.push_str(&format!(
        "if sudo -n -u {q} true 2>/dev/null; then echo 'sudo.ok=1'; else echo 'sudo.ok=0'; fi; "
    ));

    if let Some(brain) = &install {
        emit(Some(sink), StreamEvent::Status(format!("Installing {brain} on {vps_id}…")));
        script.push_str(&install_snippet(brain, &q));
    }
    script.push_str("echo provision.done");

    let sink_owned = sink.clone();
    let (code, out, err) = match run_there(
        &ctx.db,
        &vps_id,
        None,
        &script,
        Duration::from_secs(900),
        move |line| {
            if !line.trim().is_empty() {
                emit(Some(&sink_owned), StreamEvent::Status(line));
            }
        },
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };

    if code != 0 && !out.contains("provision.done") {
        return format!(
            "error: provisioning failed on {vps_id} (exit {code}): {}",
            err.trim().chars().take(600).collect::<String>()
        );
    }

    // Only remembered once the account really exists. A setting written ahead of the
    // account would send every later run at `sudo: unknown user`.
    if let Err(e) = ctx.db.set_setting(&agent_user_key(&vps_id), &user) {
        return format!("error: provisioned, but could not remember the account: {e}");
    }

    let mut report = format!(
        "Provisioned '{user}' on {vps_id}. Agent CLI runs there will use that account, \
         not root.\n"
    );
    if out.contains("sudo.ok=0") {
        report.push_str(
            "WARNING: this login cannot `sudo -n -u` to that account — unattended runs \
             will fail. Add a NOPASSWD sudoers rule for it on that server.\n",
        );
    }
    if let Some(brain) = &install {
        report.push_str(&format!("Installed {brain} (see the lines above for what it printed).\n"));
    }
    report.push_str(&format!(
        "\nStill to do, by a person, once: `{}` on that server. It opens a browser or \
         prints a device code, so it cannot be done from here or by you. Until then \
         agent_run_cli will fail on this box — confirm with agent_cli_status rather than \
         assuming.\n",
        login_hint(install.as_deref().unwrap_or("claude_code"), &user)
    ));
    report
}

/// The vendor's own installer, run as the agent account.
///
/// Piping a script from the network into a shell is what both projects document; it is
/// spelled out here rather than hidden so the approval prompt shows what will happen.
fn install_snippet(brain: &str, quoted_user: &str) -> String {
    match brain {
        "grok_cli" => format!(
            "sudo -n -u {quoted_user} -H bash -lc 'curl -fsSL https://grok.com/install.sh | bash' \
             && echo 'install.grok=1'; "
        ),
        _ => format!(
            "sudo -n -u {quoted_user} -H bash -lc 'curl -fsSL https://claude.ai/install.sh | bash' \
             && echo 'install.claude=1'; "
        ),
    }
}

// ---- agent_run_cli -----------------------------------------------------------------

async fn run_cli(ctx: &ToolContext, args: &Value, sink: &EventSink) -> String {
    let vps_id = match vps_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("").trim();
    if task.is_empty() {
        return "error: task is required — the remote agent sees nothing else, so an empty \
                brief is an agent with no instructions."
            .into();
    }
    let brain = args
        .get("brain")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("claude_code")
        .to_string();
    if !BRAINS.contains(&brain.as_str()) {
        return format!(
            "error: '{brain}' cannot be driven remotely — its non-interactive flags are \
             not verified, and guessing them produces a run that fails on a usage \
             message. Supported: {}.",
            BRAINS.join(", ")
        );
    }
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let model = args
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let timeout = Duration::from_secs(
        args.get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(900)
            .clamp(60, 7200),
    );
    let resume = args.get("resume").and_then(|v| v.as_bool()).unwrap_or(false);

    let user = agent_user(&ctx.db, &vps_id);
    if user.is_none() {
        return format!(
            "error: no agent account is provisioned on {vps_id}, so this would run the \
             CLI as the SSH login — root, on most of these hosts. Run agent_cli_provision \
             on that server first."
        );
    }

    if let Err(e) = crate::ai::safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &ctx.safety,
        &ctx.session_id,
        Some(&vps_id),
        &format!("run {brain} on {vps_id}: {}", first_line(task, 160)),
    )
    .await
    {
        return format!("error: {e}");
    }

    // The session key is the box and the directory, not this chat: a follow-up job in
    // the same checkout should continue the same remote thread even when it is asked for
    // in a different conversation.
    let session_key = format!(
        "cli-agent:{vps_id}:{}",
        cwd.as_deref().unwrap_or("~")
    );
    let previous = resume
        .then(|| crate::ai::providers::cli::get_cli_conversation(&session_key))
        .flatten();
    if resume && previous.is_none() {
        emit(Some(sink),
            StreamEvent::Status(format!(
                "No previous {brain} session on {vps_id} to resume — starting a new one."
            )),
        );
    }

    emit(Some(sink),
        StreamEvent::Status(format!("Starting {brain} on {vps_id}…")),
    );

    let bin = shell_quote(brain_bin(&brain));
    let flags = remote_flags(&brain, model.as_deref(), previous.as_deref(), task);
    let quoted: Vec<String> = flags.iter().map(|f| shell_quote(f)).collect();
    let cd = cwd
        .as_deref()
        .map(|d| format!("cd {} && ", shell_quote(d)))
        .unwrap_or_default();
    // Only Claude Code takes `--mcp-config`; Grok configures MCP servers through its own
    // `grok mcp add`, which writes them to a config file rather than accepting one per
    // run. So a Grok job gets the box's own tools and not xConsole's, and the brief
    // should not promise otherwise.
    let bridge_wanted = brain == "claude_code";

    let bridge_server = if bridge_wanted {
        let data_dir = ctx
            .home
            .0
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| ctx.home.0.clone());
        let session = crate::mcp::server_session_for_bridge(
            ctx.db.clone(),
            &data_dir,
            ctx.targets.clone(),
            ctx.safety.clone(),
            ctx.workspace_id.clone().unwrap_or_default(),
        );
        match crate::mcp::http::serve(session).await {
            Ok(s) => Some(s),
            Err(e) => {
                emit(Some(sink),
                    StreamEvent::Status(format!(
                        "Running without xConsole's tools bridged: {e}"
                    )),
                );
                None
            }
        }
    } else {
        None
    };
    let bridge = bridge_server
        .as_ref()
        .map(|b| crate::ssh::agent_exec::McpBridge {
            local_port: b.port,
            token: b.token.clone(),
        });

    let out_cell = Arc::new(std::sync::Mutex::new(ChatResponse::default()));
    let plain = Arc::new(std::sync::Mutex::new(String::new()));
    let cancel = Arc::new(AtomicBool::new(false));
    let is_stream_json = brain == "claude_code";
    let sink_owned = sink.clone();
    let out_lines = out_cell.clone();
    let plain_lines = plain.clone();
    let key = session_key.clone();
    let cd_for_cmd = cd.clone();

    let fut = crate::ssh::agent_exec::run_agent(
        &ctx.db,
        &vps_id,
        user.as_deref(),
        bridge.as_ref(),
        // Claude Code's `-p` reads the prompt from stdin; Grok's `-p` *is* the prompt and
        // takes it as an argument, so the brief is already in the flags for that one.
        if is_stream_json { task } else { "" },
        |mcp| {
            let mut cmd = format!("{cd_for_cmd}{bin} {}", quoted.join(" "));
            crate::diag(&format!("cli_agent: {cmd}"));
            if let Some(h) = mcp {
                cmd.push_str(" --mcp-config ");
                cmd.push_str(h.config_path);
            }
            cmd
        },
        Some(cancel.clone()),
        move |line| {
            if is_stream_json {
                let mut out = out_lines.lock().unwrap();
                crate::ai::providers::cli::parse_claude_code_stream_line(
                    &line,
                    &key,
                    &mut out,
                    Some(&sink_owned),
                );
            } else {
                let mut buf = plain_lines.lock().unwrap();
                buf.push_str(&line);
                buf.push('\n');
            }
        },
    );

    let run = match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return format!("error: {e}"),
        Err(_) => {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            return format!(
                "error: {brain} on {vps_id} was still running after {}s and was stopped. \
                 Whatever it had already written to disk is still there — check the \
                 working tree before starting again, and split the job or raise \
                 timeout_secs.",
                timeout.as_secs()
            );
        }
    };

    let answer = if is_stream_json {
        Arc::try_unwrap(out_cell)
            .map(|m| m.into_inner().unwrap())
            .unwrap_or_default()
            .content
    } else {
        plain.lock().unwrap().clone()
    };

    if run.exit_code != 0 && answer.trim().is_empty() {
        let detail = run.stderr.trim();
        if crate::ssh::agent_exec::is_sudo_denied(detail) {
            return format!(
                "error: could not run as the agent account on {vps_id}: {}. Fix the \
                 NOPASSWD sudoers rule there, or re-run agent_cli_provision.",
                detail.lines().next().unwrap_or("sudo refused")
            );
        }
        if crate::ssh::agent_exec::is_command_not_found(detail) {
            return format!(
                "error: {} is not on the agent account's PATH on {vps_id}. Run \
                 agent_cli_status to see what is actually installed there — an SSH \
                 command does not get the PATH you see when you log in by hand, so \
                 \"it works when I ssh in\" is not evidence.",
                brain_bin(&brain)
            );
        }
        return format!(
            "error: {brain} exited {} on {vps_id}: {}",
            run.exit_code,
            if detail.is_empty() {
                "it printed nothing. If it has never been signed in there, that is the \
                 usual cause — check agent_cli_status."
            } else {
                detail
            }
        );
    }

    if answer.trim().is_empty() {
        return format!(
            "{brain} finished on {vps_id} with exit {} and no report. It may still have \
             changed files there — verify with run_command before assuming nothing \
             happened.",
            run.exit_code
        );
    }

    format!(
        "{brain} on {vps_id} finished (exit {}).\n\n{answer}\n\n\
         This is the remote agent's own account of what it did. It has not been checked: \
         verify the claims that matter before reporting them as done.",
        run.exit_code
    )
}

/// Non-interactive flags for one remote job.
///
/// Every flag here was read off `--help` for the CLI in question, not inferred:
/// `claude --help` lists `--permission-mode` with the exact set of values used below,
/// and `grok --help` documents `-p/--single`, `--output-format plain`, `--permission-mode`
/// and `-r/--resume`. Nothing is guessed, because a guessed flag is a run that fails
/// after several minutes on a usage message.
pub fn remote_flags(
    brain: &str,
    model: Option<&str>,
    resume_id: Option<&str>,
    task: &str,
) -> Vec<String> {
    match brain {
        "grok_cli" => {
            // Grok's -p takes the prompt as its value rather than reading stdin.
            let mut a = vec!["-p".to_string(), task.to_string()];
            a.push("--output-format".into());
            a.push("plain".into());
            // Nobody is at a prompt on the far side, so a mode is always passed: the
            // alternative is a run that denies its own tool calls and reports the
            // refusals as its conclusion.
            a.push("--permission-mode".into());
            a.push("dontAsk".into());
            if let Some(m) = model {
                a.push("--model".into());
                a.push(m.to_string());
            }
            if let Some(id) = resume_id {
                a.push("--resume".into());
                a.push(id.to_string());
            }
            a
        }
        _ => {
            let mut a = vec![
                "-p".to_string(),
                "--output-format".into(),
                "stream-json".into(),
                // stream-json refuses to run without it.
                "--verbose".into(),
                "--permission-mode".into(),
                "dontAsk".into(),
            ];
            if let Some(m) = model {
                a.push("--model".into());
                a.push(m.to_string());
            }
            if let Some(id) = resume_id {
                a.push("--resume".into());
                a.push(id.to_string());
            }
            a
        }
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_clis_whose_flags_were_verified_can_be_driven() {
        // The rule that keeps this honest: a brain is listed only after its
        // non-interactive flags were read off `--help` on a machine that has it.
        assert_eq!(BRAINS, ["claude_code", "grok_cli"]);
        assert!(is_cli_agent_tool("agent_run_cli"));
        assert!(!is_cli_agent_tool("run_command"));
    }

    #[test]
    fn asking_what_is_installed_is_not_a_change_to_the_server() {
        assert!(!tool_is_mutating("agent_cli_status"));
        assert!(tool_is_mutating("agent_run_cli"));
        assert!(tool_is_mutating("agent_cli_provision"));
    }

    #[test]
    fn claude_code_is_driven_through_stdin_and_grok_through_its_prompt_argument() {
        // `claude -p` reads the prompt from stdin; `grok -p <PROMPT>` takes it as the
        // flag's value. Getting this backwards is a run that waits forever for input.
        let c = remote_flags("claude_code", None, None, "fix the build");
        assert_eq!(c[0], "-p");
        assert!(!c.contains(&"fix the build".to_string()));
        assert!(c.contains(&"stream-json".to_string()));
        assert!(c.contains(&"--verbose".to_string()), "stream-json refuses to run without it");

        let g = remote_flags("grok_cli", None, None, "fix the build");
        assert_eq!(g[0], "-p");
        assert_eq!(g[1], "fix the build");
        assert!(g.contains(&"plain".to_string()));
    }

    #[test]
    fn a_permission_mode_is_always_passed_and_is_one_the_cli_accepts() {
        // Verified against `claude --help` (acceptEdits, auto, bypassPermissions,
        // manual, dontAsk, plan) and `grok --help` (default, acceptEdits, auto, dontAsk,
        // bypassPermissions, plan). Manual is the default for -p, and nobody is at a
        // prompt on the far side.
        for brain in BRAINS {
            let f = remote_flags(brain, None, None, "x");
            let i = f.iter().position(|a| a == "--permission-mode").expect("a mode is always set");
            assert_eq!(f[i + 1], "dontAsk", "{brain}");
        }
    }

    #[test]
    fn a_model_and_a_resume_id_are_passed_through_when_given() {
        let f = remote_flags("claude_code", Some("opus"), Some("sess-1"), "x");
        assert!(f.windows(2).any(|w| w[0] == "--model" && w[1] == "opus"));
        assert!(f.windows(2).any(|w| w[0] == "--resume" && w[1] == "sess-1"));
        let f = remote_flags("grok_cli", Some("grok-4"), Some("sess-2"), "x");
        assert!(f.windows(2).any(|w| w[0] == "--model" && w[1] == "grok-4"));
        assert!(f.windows(2).any(|w| w[0] == "--resume" && w[1] == "sess-2"));
    }

    #[test]
    fn an_account_name_is_rejected_rather_than_quietly_rewritten() {
        // Renaming what the user asked for leaves a box whose agent account is not the
        // one they think it is.
        assert!(valid_user("xconsole-agent"));
        assert!(valid_user("agent_1"));
        assert!(!valid_user(""));
        assert!(!valid_user("Agent"));
        assert!(!valid_user("agent; rm -rf /"));
        assert!(!valid_user("-agent"));
        assert!(!valid_user(&"a".repeat(33)));
    }

    #[test]
    fn signing_in_is_described_as_something_only_a_person_can_do() {
        // Both are device/browser flows. A tool that claimed to automate this would
        // report success on a server that still cannot run anything.
        assert!(login_hint("claude_code", "agent").contains("setup-token"));
        assert!(login_hint("grok_cli", "agent").contains("--device-auth"));
        for brain in BRAINS {
            assert!(login_hint(brain, "agent").starts_with("sudo -u agent -H"));
        }
    }

    #[test]
    fn the_status_tool_knows_where_each_cli_keeps_its_credentials() {
        assert!(credential_paths("claude_code").iter().any(|p| p.contains(".claude")));
        assert!(credential_paths("grok_cli").iter().any(|p| p.contains(".grok")));
        assert!(credential_paths("codex_cli").is_empty());
    }

    #[test]
    fn every_tool_here_says_when_not_to_use_it() {
        // The house style, and the part a model actually acts on: a description that
        // only says what a tool does gets called for jobs it is wrong for.
        for def in definitions() {
            let d = def.description.to_lowercase();
            assert!(
                d.contains("do not use") || d.contains("cannot do") || d.contains("read-only"),
                "{} never says when not to use it",
                def.name
            );
        }
    }
}
