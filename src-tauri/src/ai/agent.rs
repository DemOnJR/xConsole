//! The agent turn loop: build the system prompt, call the provider, execute any
//! tool calls, and continue until the model stops. One loop, provider-agnostic.

use crate::ai::context::{self, PromptContext};
use crate::ai::context_compact;
use crate::ai::context_usage;
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

    let mut last = ChatMessage::assistant("");

    for iter in 0..MAX_ITERS {
        // User pressed Stop — halt before the next model call.
        if tc.session_state.is_cancelled(&tc.session_id) {
            emit(Some(sink), StreamEvent::Status("Stopped.".into()));
            break;
        }
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
