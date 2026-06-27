//! The agent turn loop: build the system prompt, call the provider, execute any
//! tool calls, and continue until the model stops. One loop, provider-agnostic.

use crate::ai::context::{self, PromptContext};
use crate::ai::context_compact;
use crate::ai::context_usage;
use crate::ai::hooks;
use crate::ai::provider::{emit, ChatMessage, ChatRequest, EventSink, StreamEvent};
use crate::ai::tools::{self, ToolContext};
use crate::ai::vps_snapshot;
use crate::ai::registry;
use serde_json::json;
use tauri::{Emitter, Manager};

/// Maximum tool-execution iterations before we stop.
const MAX_ITERS: usize = 12;

/// Run one full agent turn, streaming events to `sink`. Returns the final
/// assistant message (with any tool calls it issued).
pub async fn run_turn(
    tc: &ToolContext,
    provider_id: Option<String>,
    messages: Vec<ChatMessage>,
    conversation: bool,
    sink: &EventSink,
) -> Result<ChatMessage, String> {
    let mut messages = context::compress_window(messages);

    // Per-workspace agent status (working / planning / testing / idle) shown on the
    // workspace row. No-op when the turn isn't tied to a workspace.
    let emit_ws = |status: &str| {
        if let Some(ws) = tc.workspace_id.as_deref().filter(|s| !s.is_empty()) {
            let _ = tc.app.emit(
                "agent://workspace-status",
                json!({ "workspace_id": ws, "status": status }),
            );
        }
    };
    emit_ws(if tc.plan_mode { "planning" } else { "working" });

    let preferred_id = registry::active_provider_id(&tc.db, provider_id.as_deref())?;
    let (resolved, fallback_note) = registry::resolve_for_turn(&tc.db, &preferred_id)?;
    if let Some(note) = &fallback_note {
        emit(Some(sink), StreamEvent::Status(note.clone()));
    }
    let tool_defs = tools::definitions(&tc.home);
    let cli_mode = resolved.provider.is_autonomous_cli();
    let ollama_mode = resolved.kind == "ollama";
    // Read num_ctx from the resolved provider (not preferred_id) so a CLI→Ollama
    // fallback budgets context against the Ollama provider that actually runs.
    let ollama_num_ctx = resolved.ollama_num_ctx;
    let last_user_msg = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // UserPromptSubmit hooks: fire before the turn runs. A hook can inject extra context
    // (appended to the system prompt below) or block the turn outright (exit 2 /
    // `decision:block` / `continue:false`). Only runs when something subscribes.
    let mut hook_user_context: Option<String> = None;
    if tc.hooks.has_event(hooks::HookEvent::UserPromptSubmit) {
        let cwd = hooks::cwd();
        let input = hooks::HookEventInput {
            event: hooks::HookEvent::UserPromptSubmit,
            session_id: &tc.session_id,
            cwd: &cwd,
            workspace_id: tc.workspace_id.as_deref(),
            vps_targets: &tc.targets,
            tool_name: None,
            tool_input: None,
            tool_response: None,
            prompt: Some(&last_user_msg),
        };
        let decision = hooks::run_event(&tc.hooks, &input).await;
        if let Some(msg) = &decision.system_message {
            emit(Some(sink), StreamEvent::Status(msg.clone()));
        }
        if decision.blocks() {
            let reason = decision
                .reason
                .unwrap_or_else(|| "blocked by a UserPromptSubmit hook".to_string());
            emit(Some(sink), StreamEvent::Error(reason.clone()));
            emit_ws("idle");
            return Err(reason);
        }
        hook_user_context = decision.additional_context;
    }

    let effective_intent = vps_snapshot::effective_user_intent(&messages);
    let casual_turn = vps_snapshot::is_casual_chat(&last_user_msg);
    let needs_live = vps_snapshot::needs_live_data(&messages);
    let targeted_check = vps_snapshot::is_targeted_check(&effective_intent);
    let wants_snapshot = vps_snapshot::should_collect_snapshot(&effective_intent);
    let target_selection_note =
        vps_snapshot::target_selection_note(&effective_intent, tc.targets.len());
    if tc.targets.len() < 2 && vps_snapshot::user_asks_multiple_targets(&effective_intent) {
        emit(
            Some(sink),
            StreamEvent::Status(
                "Only 1 VPS target is selected — select both in the agent target picker to check all servers."
                    .into(),
            ),
        );
    }
    let mut snapshot = String::new();
    let mut live_command = String::new();
    if !tc.targets.is_empty() && !casual_turn && needs_live {
        if targeted_check {
            if ollama_mode {
                if let Some(cmd) = vps_snapshot::infer_live_command(&messages) {
                    live_command =
                        vps_snapshot::collect_live_command(tc, &cmd, sink).await;
                }
            }
        } else if wants_snapshot {
            snapshot = vps_snapshot::collect(tc, sink).await;
            if ollama_mode {
                if let Some(cmd) = vps_snapshot::infer_live_command(&messages) {
                    if vps_snapshot::live_command_adds_beyond_snapshot(&cmd) {
                        live_command =
                            vps_snapshot::collect_live_command(tc, &cmd, sink).await;
                    }
                }
            }
        } else if ollama_mode {
            if let Some(cmd) = vps_snapshot::infer_live_command(&messages) {
                live_command = vps_snapshot::collect_live_command(tc, &cmd, sink).await;
            }
        }
    }
    // Voice turns keep the same curated tool set as any local turn — web_search /
    // web_fetch / geo_locate are ALWAYS included (so "what's the weather?" works), plus
    // local_* tools, plus VPS tools when targets are selected. The voice prompt stays
    // fast by trimming PROSE (see voice_tiers), not by removing the agent's hands.
    let tool_defs_for_turn = if ollama_mode {
        tools::definitions_for_ollama(&tc.home, tc.targets.len(), casual_turn)
    } else {
        tool_defs.clone()
    };
    if cli_mode && !tc.targets.is_empty() && resolved.kind != "cursor" {
        let hint = "Add an OpenAI or Anthropic provider in Settings → Providers to execute \
             commands on your VPS. OpenCode/Codex CLI cannot SSH directly.";
        emit(Some(sink), StreamEvent::Error(hint.to_string()));
        emit_ws("idle");
        return Err(hint.to_string());
    }

    let data_dir = tc
        .app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"));

    let xconsole_exec = if resolved.kind == "cursor" && !tc.targets.is_empty() {
        Some(crate::ai::provider::XConsoleExec {
            data_dir: data_dir.clone(),
            session_id: tc.session_id.clone(),
            targets: tc.targets.clone(),
            safety: tc.safety.clone(),
            workspace_id: tc.workspace_id.clone().unwrap_or_default(),
        })
    } else {
        None
    };

    let mut snapshot_cli = String::new();
    if cli_mode && !tc.targets.is_empty() {
        if let Some(last_user) = messages.iter().rev().find(|m| m.role == "user") {
            if vps_snapshot::should_collect_snapshot(&last_user.content) {
                snapshot_cli = vps_snapshot::collect(tc, sink).await;
            }
        }
    }

    let conversation_summary = tc
        .db
        .get_agent_conversation(&tc.session_id)
        .ok()
        .flatten()
        .and_then(|c| c.summary)
        .filter(|s| !s.trim().is_empty());

    let ollama_provider_label = format!("{} (Ollama local)", resolved.name);
    let provider_label: &str = if ollama_mode {
        &ollama_provider_label
    } else {
        &resolved.name
    };

    let mut thread_summary = conversation_summary.clone();

    // Per-workspace project context (brief + scoped memory + the project's own
    // CLAUDE.md/AGENTS.md), loaded once for this turn when a workspace is active.
    let workspace_block = match &tc.workspace_id {
        Some(id) if !id.is_empty() && !casual_turn => {
            crate::ai::workspace_context::build_workspace_block(&tc.home, &tc.db, &tc.sessions, id)
                .await
        }
        _ => None,
    };

    // Live canvas: the terminals / SFTP panels the user has open right now, with a
    // tail of each terminal's scrollback so the agent can see what's on screen.
    // Always include it when panels are open (even on casual turns) — the user
    // expects the agent to be aware of what's on their canvas.
    let canvas_block = crate::ai::canvas_context::build_canvas_block(&tc.canvas, &tc.sessions);
    if canvas_block.is_some() {
        let n = tc
            .canvas
            .iter()
            .filter(|c| c.kind == "terminal" || c.kind == "sftp")
            .count();
        emit(
            Some(sink),
            StreamEvent::Status(format!("Looking at your open canvas ({n} panel(s))…")),
        );
    }

    let build_system = |force_minimal: bool, summary: &Option<String>| -> (String, String) {
        let ctx = PromptContext {
            home: &tc.home,
            db: &tc.db,
            model_label: &resolved.model,
            provider_label,
            safety: &tc.safety,
            target_count: tc.targets.len(),
            conversation_summary: summary.clone(),
            has_tools: !tool_defs_for_turn.is_empty(),
            vps_tools_only: ollama_mode,
            ollama_num_ctx,
            target_ids: &tc.targets,
            casual_turn,
            target_selection_note: target_selection_note.clone(),
            force_minimal_prompt: force_minimal,
            plan_mode: tc.plan_mode,
            workspace_context: workspace_block.clone(),
            canvas_context: canvas_block.clone(),
            conversation,
        };

        if cli_mode {
            let mut base = vps_snapshot::build_cli_system(
                &resolved.name,
                &resolved.model,
                tc.targets.len(),
                &snapshot_cli,
                summary.as_deref(),
            );
            if xconsole_exec.is_some() {
                base.push_str(
                    "\n\nYou have xConsole MCP tools for the user's VPS: run_command, read_file, \
                     write_file, list_vps_targets, skills_list, skill_view, skill_save, memory_save, \
                     set_project_brief. \
                     You ALSO control the user's canvas: canvas_open_terminal and canvas_open_sftp open \
                     a live panel for a server, canvas_close removes a panel (node_id or vps_id), \
                     canvas_refresh reconnects a terminal, and canvas_tile arranges them. So when the \
                     user asks to open/duplicate/close a terminal or panel on the canvas, CALL the \
                     matching canvas_* tool — never reply that you can't open canvas panels. \
                     Use them to inspect and change servers — do not tell the user to run commands \
                     manually. Call list_vps_targets first if you need host ids. Load relevant skills \
                     (skill_view) before complex infra work.",
                );
            }
            // The CLI system prompt is built from scratch, so the shared context
            // tiers (workspace + live canvas) are appended explicitly here.
            if let Some(ws) = &workspace_block {
                base.push_str("\n\n");
                base.push_str(ws);
            }
            if let Some(cv) = &canvas_block {
                base.push_str("\n\n");
                base.push_str(cv);
            }
            return (base, String::new());
        }

        if ollama_mode {
            if ollama_num_ctx.is_some_and(|n| n < 65_536) && !force_minimal {
                emit(
                    Some(sink),
                    StreamEvent::Status(
                        "Using compact prompt for local model (context under 64K). \
                         Increase context to 64K+ in Settings → Providers for full agent memory."
                            .into(),
                    ),
                );
            }
            let mut snap_txt = String::new();
            let mut system = context::build_system_prompt(&ctx);
            if !snapshot.is_empty() {
                let ctx_budget = if force_minimal {
                    ollama_num_ctx.unwrap_or(65_536).min(32_768)
                } else {
                    ollama_num_ctx.unwrap_or(65_536)
                };
                snap_txt = vps_snapshot::truncate_for_context(&snapshot, ctx_budget);
                system.push_str("\n\n");
                system.push_str(&snap_txt);
            }
            if !live_command.is_empty() {
                system.push_str("\n\n");
                system.push_str(&live_command);
            }
            return (system, snap_txt);
        }

        (context::build_system_prompt(&ctx), String::new())
    };

    let (mut system, mut snapshot_text) = build_system(false, &thread_summary);

    let context_limit =
        context_usage::default_context_limit(&resolved.kind, ollama_num_ctx);

    if registry::is_tool_capable_kind(&resolved.kind) && !cli_mode {
        if let Ok(Some(compact)) = context_compact::auto_compact_if_needed(
            &mut messages,
            &system,
            &tool_defs_for_turn,
            context_limit,
            thread_summary.as_deref(),
            resolved.provider.as_ref(),
            &resolved.model,
            Some(sink),
        )
        .await
        {
            emit(
                Some(sink),
                StreamEvent::Status(format!(
                    "Compacted context: ~{} → ~{} tokens ({} tool result(s) pruned)",
                    compact.tokens_before, compact.tokens_after, compact.pruned_tools
                )),
            );
            thread_summary = Some(compact.summary);
            emit(
                Some(sink),
                StreamEvent::ConversationCompacted {
                    messages: messages.clone(),
                },
            );
        }
    }

    let mut usage = context_usage::compute_usage(
        &PromptContext {
            home: &tc.home,
            db: &tc.db,
            model_label: &resolved.model,
            provider_label,
            safety: &tc.safety,
            target_count: tc.targets.len(),
            conversation_summary: thread_summary.clone(),
            has_tools: !tool_defs_for_turn.is_empty(),
            vps_tools_only: ollama_mode,
            ollama_num_ctx,
            target_ids: &tc.targets,
            casual_turn,
            target_selection_note: target_selection_note.clone(),
            force_minimal_prompt: false,
            plan_mode: tc.plan_mode,
            workspace_context: workspace_block.clone(),
            canvas_context: canvas_block.clone(),
            conversation,
        },
        &tool_defs_for_turn,
        &messages,
        &snapshot_text,
        &live_command,
        &resolved.kind,
    );

    if context_compact::force_minimal_system_prompt(usage.total_tokens, context_limit) {
        emit(
            Some(sink),
            StreamEvent::Status(
                "Context tight — switching to ponytail-minimal system prompt.".into(),
            ),
        );
        let rebuilt = build_system(true, &thread_summary);
        system = rebuilt.0;
        snapshot_text = rebuilt.1;
        usage = context_usage::compute_usage(
            &PromptContext {
                home: &tc.home,
                db: &tc.db,
                model_label: &resolved.model,
                provider_label,
                safety: &tc.safety,
                target_count: tc.targets.len(),
                conversation_summary: thread_summary.clone(),
                has_tools: !tool_defs_for_turn.is_empty(),
                vps_tools_only: ollama_mode,
                ollama_num_ctx,
                target_ids: &tc.targets,
                casual_turn,
                target_selection_note: target_selection_note.clone(),
                force_minimal_prompt: true,
                plan_mode: tc.plan_mode,
                workspace_context: workspace_block.clone(),
                canvas_context: canvas_block.clone(),
                conversation,
            },
            &tool_defs_for_turn,
            &messages,
            &snapshot_text,
            &live_command,
            &resolved.kind,
        );
    }
    emit(
        Some(sink),
        StreamEvent::ContextUsage(crate::ai::provider::ContextUsageEvent {
            segments: usage
                .segments
                .into_iter()
                .map(|s| crate::ai::provider::ContextUsageSegment {
                    key: s.key,
                    label: s.label,
                    tokens: s.tokens,
                })
                .collect(),
            total_tokens: usage.total_tokens,
            context_limit: usage.context_limit,
            percent: usage.percent,
        }),
    );

    // Fold in any context a UserPromptSubmit hook injected, so the model sees it this turn.
    if let Some(extra) = &hook_user_context {
        system.push_str("\n\n## Additional context (from a UserPromptSubmit hook)\n");
        system.push_str(extra);
    }

    // ---- Capability-gap autopilot (autoresearch) -------------------------
    // A weak local model won't reliably pick learn_skill itself (measured: trigger
    // recall ~0 across prompt wordings), but it answers a focused YES/NO-style classifier
    // reliably (recall ~0.75, zero false positives). So before the turn we run one cheap
    // classification; on a detected gap with no covering skill we research it and inject
    // the resulting skill here, so the model applies it THIS turn — acknowledging and
    // building the skill automatically instead of guessing. Gated to local tool turns
    // and `agent.learn_autopilot` (default on); the expensive research only runs on a
    // genuine detected gap.
    // When the autopilot applies a researched skill this turn, its name is held here so
    // the turn's outcome (clean vs troubled) can update the skill's verified status.
    let mut autopilot_skill: Option<String> = None;
    let learn_autopilot = tc
        .db
        .get_setting("agent.learn_autopilot")
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    if learn_autopilot
        && ollama_mode
        && !cli_mode
        && !casual_turn
        && !conversation
        && !tool_defs_for_turn.is_empty()
        && !last_user_msg.trim().is_empty()
    {
        let installed: Vec<String> = crate::ai::skills::discover(&tc.home)
            .into_iter()
            .map(|s| {
                if s.description.is_empty() {
                    s.name.replace('-', " ")
                } else {
                    format!("{} ({})", s.name.replace('-', " "), s.description)
                }
            })
            .collect();
        if let Some(topic) = crate::ai::autoresearch::assess_gap(
            resolved.provider.as_ref(),
            &resolved.model,
            &last_user_msg,
            &installed,
        )
        .await
        {
            let known_hosts: Vec<String> = tc
                .targets
                .iter()
                .filter_map(|id| tc.db.get_vps(id).ok().flatten())
                .flat_map(|v| [v.host, v.name])
                .collect();
            let scan_opts = crate::ai::skill_scan::scan_options_from_db(&tc.db);
            let res = crate::ai::autoresearch::learn(
                &tc.home,
                resolved.provider.as_ref(),
                &resolved.model,
                &topic,
                None,
                &known_hosts,
                None,
                &scan_opts,
                Some(sink),
            )
            .await;
            use crate::ai::autoresearch::LearnStatus;
            match res.status {
                LearnStatus::Saved | LearnStatus::Exists => {
                    // Trust the skill according to its verification status: a verified
                    // skill is applied forcefully; a draft is offered as cautious notes
                    // (so a possibly-wrong skill can't override a correct instinct); a
                    // quarantined skill is not applied at all.
                    let status = crate::ai::autoresearch::skill_status(&tc.home, &res.name);
                    match crate::ai::autoresearch::injection_block(&status, &res.body) {
                        Some(block) => {
                            emit(
                                Some(sink),
                                StreamEvent::Status(format!(
                                    "Learned a skill for \"{topic}\" ({status}) — applying it."
                                )),
                            );
                            system.push_str(&block);
                            // Record this turn's outcome against the skill at end-of-turn.
                            autopilot_skill = Some(res.name.clone());
                        }
                        None => {
                            system.push_str(
                                "\n\n# Note: the researched approach for this task is quarantined \
                                 (it failed before). Don't rely on it; tell the user you're not \
                                 certain of the exact steps.",
                            );
                        }
                    }
                }
                LearnStatus::NoSources | LearnStatus::Refused => {
                    system.push_str(
                        "\n\n# Note: a web search for this task didn't yield a reliable procedure. \
                         Tell the user honestly that you're not certain of the exact steps rather \
                         than guessing commands.",
                    );
                }
                LearnStatus::Error => {}
            }
        }
    }

    let mut last = ChatMessage::assistant("");
    let mut iters_used = 0usize;

    for iter in 0..MAX_ITERS {
        // User pressed Stop — halt before the next model call.
        if tc.session_state.is_cancelled(&tc.session_id) {
            emit(Some(sink), StreamEvent::Status("Stopped.".into()));
            break;
        }
        iters_used = iter + 1;
        let mut req = ChatRequest::new(&resolved.model);
        req.system = system.clone();
        req.messages = messages.clone();
        req.tools = tool_defs_for_turn.clone();
        req.xconsole = xconsole_exec.clone();
        // Let the provider's stream loop abort the moment the user presses Stop.
        req.cancel = Some(tc.session_state.cancel_flag(&tc.session_id));

        let resp = match resolved.provider.chat(&req, Some(sink)).await {
            Ok(r) => r,
            Err(e) => {
                emit(Some(sink), StreamEvent::Error(e.clone()));
                emit_ws("idle");
                return Err(e);
            }
        };

        let assistant = ChatMessage {
            role: "assistant".into(),
            content: resp.content.clone(),
            tool_calls: resp.tool_calls.clone(),
            tool_call_id: None,
        };
        messages.push(assistant.clone());
        last = assistant;

        // No tools to run, or an autonomous CLI that does its own tool use.
        if resp.tool_calls.is_empty() || cli_mode {
            break;
        }
        // Surface a "testing" status when the agent runs a test/verify command.
        let testing = resp.tool_calls.iter().any(|c| {
            c.arguments
                .get("command")
                .and_then(|v| v.as_str())
                .map(|cmd| {
                    let l = cmd.to_lowercase();
                    l.contains("test") || l.contains("pytest") || l.contains("verify")
                })
                .unwrap_or(false)
        });
        // Don't start running tools if the user pressed Stop during generation.
        if tc.session_state.is_cancelled(&tc.session_id) {
            emit(Some(sink), StreamEvent::Status("Stopped.".into()));
            break;
        }
        emit_ws(if testing { "testing" } else { "working" });
        for call in &resp.tool_calls {
            // The provider already streamed StreamEvent::ToolCall for each call;
            // the single ToolResult is emitted by this loop below. No re-emit here.
            let output = tools::dispatch(tc, call, sink).await;
            emit(
                Some(sink),
                StreamEvent::ToolResult {
                    id: call.id.clone(),
                    output: output.clone(),
                },
            );
            messages.push(ChatMessage::tool_result(call.id.clone(), output));
        }

        if iter == MAX_ITERS - 1 && !resp.tool_calls.is_empty() {
            emit(
                Some(sink),
                StreamEvent::Error(format!(
                    "Agent stopped after {MAX_ITERS} tool iterations; task may be incomplete."
                )),
            );
        }
    }

    // Self-improvement loop (ETAPA 29): before finishing, look at what went wrong this
    // turn (failed/retried tool calls, hitting the iteration cap), distill a short
    // lesson, and save it to memory — where it's injected into every future turn's
    // prompt. Pure analysis runs every turn but only WRITES when there was trouble, so
    // it adds no latency to clean turns (including voice). On by default; disable with
    // the `agent.self_improve` setting = "false".
    let self_improve = tc
        .db
        .get_setting("agent.self_improve")
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    if self_improve && registry::is_tool_capable_kind(&resolved.kind) && !cli_mode {
        let lessons =
            crate::ai::reflection::reflect_and_save(&tc.home, &messages, iters_used, MAX_ITERS);
        if !lessons.is_empty() {
            emit(
                Some(sink),
                StreamEvent::Status(format!(
                    "Self-improvement: learned {} lesson(s) from this turn and saved them to memory.",
                    lessons.len()
                )),
            );
        }
    }

    // Skill verification: if the autopilot APPLIED a researched skill this turn AND the
    // agent actually acted (ran tools), record whether the turn ran clean. Clean uses
    // promote a draft to `verified`; failures eventually quarantine it — so a skill only
    // earns trust by working, and a bad one stops being applied. Knowledge-only turns
    // (no tool calls) carry no execution signal, so they don't move the status.
    if let Some(skill) = autopilot_skill {
        let acted = messages
            .iter()
            .any(|m| m.role == "assistant" && !m.tool_calls.is_empty());
        if acted {
            let outcome = crate::ai::reflection::analyze_turn(&messages, iters_used, MAX_ITERS);
            let new_status =
                crate::ai::autoresearch::record_outcome(&tc.home, &skill, !outcome.had_trouble());
            emit(
                Some(sink),
                StreamEvent::Status(format!(
                    "Skill `{skill}` {} this turn → status: {new_status}.",
                    if outcome.had_trouble() { "had trouble" } else { "ran clean" }
                )),
            );
        }
    }

    // Stop hooks: fire once the turn has finished (notifications, formatting, running
    // a test suite, etc.). xConsole doesn't force the agent to keep going, so this is
    // fire-and-forget — any message/context the hook returns is surfaced as a status.
    if tc.hooks.has_event(hooks::HookEvent::Stop) {
        let cwd = hooks::cwd();
        let input = hooks::HookEventInput {
            event: hooks::HookEvent::Stop,
            session_id: &tc.session_id,
            cwd: &cwd,
            workspace_id: tc.workspace_id.as_deref(),
            vps_targets: &tc.targets,
            tool_name: None,
            tool_input: None,
            tool_response: None,
            prompt: None,
        };
        let decision = hooks::run_event(&tc.hooks, &input).await;
        if let Some(msg) = decision
            .system_message
            .or(decision.additional_context)
            .or(decision.reason)
        {
            emit(Some(sink), StreamEvent::Status(format!("Stop hook: {msg}")));
        }
    }

    emit(Some(sink), StreamEvent::Done);
    emit_ws("idle");

    if last.content.trim().is_empty() && !last.tool_calls.is_empty() {
        // Tool loop will continue on the next iteration; no placeholder needed.
    } else if ollama_mode
        && last.tool_calls.is_empty()
        && last.content.len() < 25
        && !casual_turn
        && (vps_snapshot::should_collect(&last_user_msg) || !snapshot.is_empty())
    {
        let ctx_hint = ollama_num_ctx
            .map(|n| format!(" (context: {n})"))
            .unwrap_or_default();
        last.content = format!(
            "The local model returned a truncated reply{ctx_hint}: \"{}\". \
             VPS snapshots + tools need at least 64K context — raise it in Settings → Providers.",
            last.content.trim()
        );
    } else if last.content.trim().is_empty() {
        if ollama_mode {
            let ctx_hint = ollama_num_ctx
                .map(|n| format!(" (context: {n})"))
                .unwrap_or_default();
            last.content = format!(
                "The local model returned an empty or truncated reply{ctx_hint}. \
                 With VPS snapshots + tools, use at least 64K context in Settings → Providers. \
                 If replies stop after one word, the prompt is too large for your context setting."
            );
        } else if !snapshot.is_empty() {
            last.content = "I pulled live data from your VPS (see activity above) but the model \
                            returned an empty reply. Try again or switch to Cursor/OpenAI for \
                            complex server questions."
                .into();
        }
    }

    Ok(last)
}
