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

// No tool-round cap. Claude / Grok / OpenAI do not limit how many tools a
// turn may run — they stop when the model returns text (or the user hits Stop).
// A 20-iter ceiling left unfinished `tool_calls` in history and the next
// request 400'd ("assistant message with tool_calls must be followed by tool messages").

/// Write cache hit/miss to `xconsole.log` + `cache.jsonl`.
///
/// Release builds set `windows_subsystem = "windows"`, so `eprintln!` is discarded.
/// The installed app's "hi / how are you" session left no cache lines in the log
/// for that reason — the numbers only survived on the assistant `tokenStats`.
fn log_prompt_cache(
    session_id: &str,
    iter: u32,
    prompt: u32,
    cached: u32,
    classification: &str,
    reason: Option<&str>,
) {
    let report = crate::ai::cost::cache_report(prompt, cached);
    let line = crate::ai::cost::format_cache_line(prompt, cached);
    crate::diag(&format!(
        "cache session={session_id} iter={iter} {line} · prefix={classification}"
    ));
    if let Some(why) = reason {
        crate::diag(why);
    }
    let payload = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "session": session_id,
        "iter": iter,
        "prompt": prompt,
        "hit": report.hit,
        "miss": report.miss,
        "pct": (report.rate * 100.0).round() as i32,
        "prefix": classification,
        "reason": reason,
    });
    crate::diag_jsonl("cache.jsonl", &payload.to_string());
}

/// Run one full agent turn, streaming events to `sink`. Returns the final
/// assistant message (with any tool calls it issued).
pub async fn run_turn(
    tc: &ToolContext,
    provider_id: Option<String>,
    messages: Vec<ChatMessage>,
    conversation: bool,
    sink: &EventSink,
) -> Result<ChatMessage, String> {
    let telemetry = crate::ai::tool_cache::new_turn_telemetry();
    let mut previous_prefix = tc.session_state.last_prefix(&tc.session_id);

    // Tool-result budget: cap what rides back into context so long command outputs
    // don't blow up every subsequent request. 0 = unlimited (opt out).
    let tool_result_max_chars: usize = tc
        .db
        .get_setting("agent.tool_result_max_chars")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    // Compress by command type first (failures, not cargo progress;
    // git hints dropped; logs deduped), then apply the hard char cap.
    let cap_tool_result = |call: &crate::ai::provider::ToolCall, output: &str| -> String {
        let cmd = crate::ai::output_compress::command_from_call(&call.name, &call.arguments);
        crate::ai::output_compress::compress_and_cap(&cmd, output, tool_result_max_chars)
    };

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
    // Local models have a small KV window so we must drop old turns. API
    // providers prompt-cache the growing prefix — a 20K sliding window rewrites
    // that prefix every turn and converts cheap cache reads into full misses.
    let mut messages = if ollama_mode {
        match ollama_num_ctx {
            Some(n) => context::compress_window_to(
                messages,
                (n as usize).saturating_sub(4_096).max(context::WORKING_SET_TOKENS),
            ),
            None => context::compress_window(messages),
        }
    } else {
        context::compress_window_to(messages, context::API_WORKING_SET_TOKENS)
    };
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
    let mut tool_defs_for_turn = if ollama_mode {
        tools::definitions_for_ollama(&tc.home, tc.targets.len(), casual_turn)
    } else {
        tool_defs.clone()
    };

    let session_url = tc
        .db
        .get_provider(&preferred_id)
        .ok()
        .flatten()
        .and_then(|p| p.base_url)
        .unwrap_or_default();
    let vision_provider_setting = tc
        .db
        .get_setting(crate::ai::vision::SETTING_PROVIDER)
        .ok()
        .flatten()
        .unwrap_or_default();
    let vision_model_setting = tc
        .db
        .get_setting(crate::ai::vision::SETTING_MODEL)
        .ok()
        .flatten()
        .unwrap_or_default();
    let vision_native = crate::ai::vision::use_native(
        &resolved.kind,
        &resolved.model,
        &session_url,
        &preferred_id,
        &vision_provider_setting,
        &vision_model_setting,
        !tc.turn_images.is_empty(),
    );
    let vision_via_tool = !tc.turn_images.is_empty() && !vision_native && !cli_mode;
    if vision_via_tool {
        tool_defs_for_turn.push(crate::ai::vision::tool_def());
        emit(
            Some(sink),
            StreamEvent::Status(format!(
                "Images attached — session model cannot see pixels; use the vision tool ({}).",
                if vision_model_setting.is_empty() {
                    "Gemini if configured"
                } else {
                    vision_model_setting.as_str()
                }
            )),
        );
    } else if vision_native {
        emit(
            Some(sink),
            StreamEvent::Status(format!(
                "Sending {} image(s) to the session model.",
                tc.turn_images.len()
            )),
        );
    }

    let data_dir = tc
        .app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"));
    tc.session_state.load_prefix_cache(&data_dir, &tc.session_id);
    if previous_prefix.is_none() {
        previous_prefix = tc.session_state.last_prefix(&tc.session_id);
    }

    let xconsole_exec = if cli_mode && !tc.targets.is_empty() {
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

    // Returns (static_system, dynamic_block, snapshot_text_for_usage).
    // Dynamic goes into the last user message of the *request* so the system
    // prefix stays byte-stable for Ollama KV reuse / Anthropic prompt caching.
    let build_system = |force_minimal: bool, summary: &Option<String>| -> (String, String, String) {
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
            todo_context: crate::ai::todos::format_block(&tc.session_state.todos(&tc.session_id)),
            conversation,
        };

        if cli_mode {
            let mut base = vps_snapshot::build_cli_system(
                &resolved.name,
                &resolved.model,
                tc.targets.len(),
                "",
                summary.as_deref(),
            );
            if xconsole_exec.is_some() {
                base.push_str(
                    "\n\nYou have xConsole MCP tools for the user's VPS: run_command, read_file, \
                     write_file, list_vps_targets, skills_list, skill_view, skill_save, memory_save, \
                     host_memory_get, host_memory_update, set_project_brief. \
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
            let mut dynamic = String::new();
            if !snapshot_cli.is_empty() {
                dynamic.push_str(&snapshot_cli);
                dynamic.push_str("\n\n");
            }
            if let Some(ws) = &workspace_block {
                dynamic.push_str(ws);
                dynamic.push_str("\n\n");
            }
            if let Some(cv) = &canvas_block {
                dynamic.push_str(cv);
                dynamic.push_str("\n\n");
            }
            if let Some(todos) = crate::ai::todos::format_block(&tc.session_state.todos(&tc.session_id)) {
                dynamic.push_str(&todos);
                dynamic.push_str("\n\n");
            }
            if !casual_turn && !tc.targets.is_empty() {
                let host_dossiers = crate::ai::host_memory::format_for_prompt(&tc.home, &tc.db, &tc.targets);
                if !host_dossiers.is_empty() {
                    dynamic.push_str(&host_dossiers);
                }
            }
            let snap_copy = snapshot_cli.clone();
            return (base, dynamic.trim().to_string(), snap_copy);
        }

        if ollama_mode
            && ollama_num_ctx.is_some_and(|n| n < 65_536)
            && !force_minimal
        {
            emit(
                Some(sink),
                StreamEvent::Status(
                    "Using compact prompt for local model (context under 64K). \
                     Increase context to 64K+ in Settings → Providers for full agent memory."
                        .into(),
                ),
            );
        }

        let assembled = context::assemble_prompt(&ctx);
        let mut dynamic = assembled.dynamic_block;
        let mut snap_txt = String::new();
        // Live canvas already shows what's on screen. A 50K-char snapshot in
        // the last user message is a permanent cache-miss tail — skip it when
        // canvas is present, and keep a small cap for API providers otherwise.
        if !snapshot.is_empty() && canvas_block.is_none() {
            let ctx_budget = if ollama_mode {
                if force_minimal {
                    ollama_num_ctx.unwrap_or(65_536).min(32_768)
                } else {
                    ollama_num_ctx.unwrap_or(65_536)
                }
            } else {
                8_192 // → ~3K chars in truncate_for_context
            };
            snap_txt = vps_snapshot::truncate_for_context(&snapshot, ctx_budget);
            if !dynamic.is_empty() {
                dynamic.push_str("\n\n");
            }
            dynamic.push_str(&snap_txt);
        }
        if !live_command.is_empty() {
            if !dynamic.is_empty() {
                dynamic.push_str("\n\n");
            }
            dynamic.push_str(&live_command);
        }
        (assembled.static_system, dynamic, snap_txt)
    };

    let (mut system, mut dynamic_block, mut snapshot_text) =
        build_system(false, &thread_summary);

    if vision_via_tool {
        if !dynamic_block.is_empty() {
            dynamic_block.push_str("\n\n");
        }
        dynamic_block.push_str(&crate::ai::vision::tool_hint(tc.turn_images.len()));
    }
    if cli_mode && !tc.turn_images.is_empty() {
        emit(
            Some(sink),
            StreamEvent::Status(format!(
                "Looking at {} image(s) with the vision model…",
                tc.turn_images.len()
            )),
        );
        match crate::ai::vision::describe_all(
            &tc.db,
            &tc.turn_images,
            "Describe this image for a coding agent. Transcribe visible text. Note UI, errors, code, and layout.",
        )
        .await
        {
            Ok(text) if !text.is_empty() => {
                if let Some(user) = messages
                    .iter_mut()
                    .rev()
                    .find(|m| m.role == "user" && !context::is_runtime_message(m))
                {
                    user.content.push_str("\n\n");
                    user.content.push_str(&text);
                }
            }
            Ok(_) => {}
            Err(e) => {
                emit(
                    Some(sink),
                    StreamEvent::Status(format!(
                        "Vision unavailable ({e}) — continuing without pixels."
                    )),
                );
            }
        }
    }

    let context_limit =
        context_usage::default_context_limit(&resolved.kind, &resolved.model, ollama_num_ctx);

    if registry::is_tool_capable_kind(&resolved.kind) && !cli_mode {
        if let Ok(Some(compact)) = context_compact::auto_compact_if_needed(
            &mut messages,
            &system,
            &tool_defs_for_turn,
            context_limit,
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
            todo_context: crate::ai::todos::format_block(&tc.session_state.todos(&tc.session_id)),
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
        dynamic_block = rebuilt.1;
        snapshot_text = rebuilt.2;
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
                todo_context: crate::ai::todos::format_block(&tc.session_state.todos(&tc.session_id)),
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

    // Fold in any context a UserPromptSubmit hook injected — dynamic block (not system).
    if let Some(extra) = &hook_user_context {
        if !dynamic_block.is_empty() {
            dynamic_block.push_str("\n\n");
        }
        dynamic_block.push_str("## Additional context (from a UserPromptSubmit hook)\n");
        dynamic_block.push_str(extra);
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
                            // Keep researched skills out of the static system prefix.
                            if !dynamic_block.is_empty() {
                                dynamic_block.push_str("\n\n");
                            }
                            dynamic_block.push_str(&block);
                            // Record this turn's outcome against the skill at end-of-turn.
                            autopilot_skill = Some(res.name.clone());
                        }
                        None => {
                            if !dynamic_block.is_empty() {
                                dynamic_block.push_str("\n\n");
                            }
                            dynamic_block.push_str(
                                "# Note: the researched approach for this task is quarantined \
                                 (it failed before). Don't rely on it; tell the user you're not \
                                 certain of the exact steps.",
                            );
                        }
                    }
                }
                LearnStatus::NoSources | LearnStatus::Refused => {
                    if !dynamic_block.is_empty() {
                        dynamic_block.push_str("\n\n");
                    }
                    dynamic_block.push_str(
                        "# Note: a web search for this task didn't yield a reliable procedure. \
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

    // Replay the last provider-visible prefix (frozen runtime blocks included)
    // so this turn is a true append. Dropping last turn's `# Runtime context`
    // and putting the new assistant in that slot was the installed-app miss:
    // turn 2 hit only 1536/8277 (system+tools+first user).
    if let Some(prev) = tc.session_state.last_request_messages(&tc.session_id) {
        if let Some(continued) = context::continue_cached_prefix(&prev, &messages) {
            messages = continued;
        }
    }
    // Freeze runtime once per user turn. Tool-loop iters must not move or
    // replace it — that would bust the prefix we just paid to write.
    if vision_native {
        crate::ai::vision::attach_images_to_latest_user(&mut messages, tc.turn_images.clone());
    }
    if !messages.last().is_some_and(context::is_runtime_message) {
        context::inject_dynamic_into_last_user(&mut messages, &dynamic_block);
    }
    // Repair history from a previous stop/cap so DeepSeek/OpenAI accept this turn.
    crate::ai::provider::close_unanswered_tool_calls(&mut messages);

    // Request settings, read once for the whole turn.
    //
    // These used to be four `get_setting` calls inside the loop below, so a turn that
    // ran 30 tool rounds took the global DB mutex 120 times to re-read values that had
    // already been decided — competing with the SSH sessions, conversation persistence
    // and the encryption persister that need the same lock. Reading them once also
    // makes the turn self-consistent: every round now uses the model, token cap and
    // reasoning level the turn started with, instead of silently switching mid-turn if
    // a setting changed underneath it.
    let turn_max_tokens: u32 = tc
        .db
        .get_setting("agent.max_tokens")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .filter(|n: &u32| *n >= 256)
        .unwrap_or(16_384);
    // Opt-in extended cache TTL (1h) when the user enables it — 2x write price
    // but the prefix survives idle gaps that would evict the 5-min cache.
    let turn_cache_retention = tc
        .db
        .get_setting("agent.cache_retention")
        .ok()
        .flatten()
        .unwrap_or_default();
    // Reasoning effort capability control: off|low|medium|high.
    let turn_reasoning = tc
        .db
        .get_setting("agent.reasoning_level")
        .ok()
        .flatten()
        .unwrap_or_default();
    // Per-chat model override (set via /model); empty falls back to the provider's
    // configured model.
    let turn_model = tc
        .db
        .get_setting("agent.active_model")
        .ok()
        .flatten()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| resolved.model.clone());

    let mut iter: usize = 0;
    let mut execution_nudge = false;
    let mut truncate_continues: u8 = 0;
    loop {
        // User pressed Stop — halt before the next model call.
        if tc.session_state.is_cancelled(&tc.session_id) {
            emit(Some(sink), StreamEvent::Status("Stopped.".into()));
            crate::ai::provider::close_unanswered_tool_calls(&mut messages);
            tc.session_state.store_request_messages(
                &tc.session_id,
                crate::ai::vision::without_images(&messages),
            );
            tc.session_state.persist_prefix_cache(&data_dir, &tc.session_id);
            break;
        }
        iters_used = iter + 1;
        crate::ai::output_compress::age_historical_tool_results(&mut messages, 4, 1500);
        let mut req = ChatRequest::new(&turn_model);
        req.system = system.clone();
        req.messages = messages.clone();
        req.tools = tool_defs_for_turn.clone();
        req.max_tokens = turn_max_tokens;
        req.xconsole = xconsole_exec.clone();
        req.cache_retention = turn_cache_retention.clone();
        // Stable per-session id for provider cache routing (OpenAI prompt_cache_key).
        req.session_id = tc.session_id.clone();
        req.reasoning = turn_reasoning.clone();
        // Let the provider's stream loop abort the moment the user presses Stop.
        req.cancel = Some(tc.session_state.cancel_flag(&tc.session_id));

        let current_prefix = crate::ai::prefix_telemetry::fingerprint_request(&req);
        let classification = crate::ai::prefix_telemetry::classify(
            previous_prefix.as_ref(),
            &current_prefix,
        );
        emit(
            Some(sink),
            StreamEvent::PrefixTelemetry(crate::ai::provider::PrefixTelemetryEvent {
                request_index: iter as u32,
                system_hash: current_prefix.system.hash.clone(),
                schema_hash: current_prefix.schema.hash.clone(),
                message_prefix_hash: current_prefix.messages.hash.clone(),
                system_bytes: current_prefix.system.bytes as u64,
                schema_bytes: current_prefix.schema.bytes as u64,
                message_bytes: current_prefix.messages.bytes as u64,
                classification: classification.as_str().into(),
                source: resolved.kind.clone(),
            }),
        );
        previous_prefix = Some(current_prefix.clone());
        tc.session_state
            .store_prefix(&tc.session_id, current_prefix);

        let resp = match resolved.provider.chat(&req, Some(sink)).await {
            Ok(r) => r,
            Err(e) => {
                crate::ai::provider::close_unanswered_tool_calls(&mut messages);
                tc.session_state.store_request_messages(
                    &tc.session_id,
                    crate::ai::vision::without_images(&messages),
                );
                tc.session_state.persist_prefix_cache(&data_dir, &tc.session_id);
                emit(Some(sink), StreamEvent::Error(e.clone()));
                emit_ws("idle");
                return Err(e);
            }
        };

        if let Some(prompt) = resp.prompt_tokens {
            let cached = resp.cached_tokens.unwrap_or(0);
            let line = crate::ai::cost::format_cache_line(prompt, cached);
            emit(Some(sink), StreamEvent::Status(line.clone()));
            let why = crate::ai::cost::cache_miss_reason(
                prompt,
                cached,
                classification.as_str(),
                iter as u32,
            );
            if let Some(reason) = &why {
                emit(Some(sink), StreamEvent::Status(reason.clone()));
            }
            log_prompt_cache(
                &tc.session_id,
                iter as u32,
                prompt,
                cached,
                classification.as_str(),
                why.as_deref(),
            );
        }

        let assistant = ChatMessage {
            role: "assistant".into(),
            content: resp.content.clone(),
            tool_calls: resp.tool_calls.clone(),
            tool_call_id: None,
            images: vec![],
            reasoning_content: resp.reasoning_content.clone(),
        };
        messages.push(assistant.clone());
        last = assistant;

        // Keep the in-memory prefix current so the next model call can append.
        // Disk is written once at turn end — rewriting the full transcript JSON
        // after every tool made a long session a multi-MB/s writer.
        tc.session_state.store_request_messages(
            &tc.session_id,
            crate::ai::vision::without_images(&messages),
        );

        // No tools to run, or an autonomous CLI that does its own tool use.
        if resp.tool_calls.is_empty() || cli_mode {
            if !cli_mode {
                // Model wrote a plan as chat text and skipped present_plan —
                // open the review modal ourselves so the user can approve.
                if let Some(call) = crate::ai::consent::synthetic_present_plan(
                    tc.plan_mode,
                    tc.session_state.plan_approved(&tc.session_id),
                    &resp.content,
                ) {
                    emit(
                        Some(sink),
                        StreamEvent::Status(
                            "Opening the plan review modal — the plan was written in chat.".into(),
                        ),
                    );
                    if let Some(asst) = messages.last_mut() {
                        if asst.role == "assistant" {
                            asst.tool_calls = vec![call.clone()];
                        }
                    }
                    last.tool_calls = vec![call.clone()];
                    emit(Some(sink), StreamEvent::ToolCall(call.clone()));
                    let output =
                        tools::dispatch_with_telemetry(tc, &call, sink, Some(&telemetry)).await;
                    let capped = cap_tool_result(&call, &output);
                    emit(
                        Some(sink),
                        StreamEvent::ToolResult {
                            id: call.id.clone(),
                            output: capped.clone(),
                        },
                    );
                    messages.push(ChatMessage::tool_result(call.id, capped));
                    iter += 1;
                    continue;
                }
                // User approved (modal or chat) but the model returned empty /
                // "waiting for you" instead of executing. Nudge once.
                if crate::ai::consent::should_nudge_execute(
                    tc.plan_mode,
                    tc.session_state.plan_approved(&tc.session_id),
                    &resp.content,
                    execution_nudge,
                ) {
                    execution_nudge = true;
                    emit(
                        Some(sink),
                        StreamEvent::Status(
                            "Plan approved — continuing execution.".into(),
                        ),
                    );
                    messages.push(ChatMessage::user(
                        "[system] The user approved the plan. Execute it now with tools. \
                         Do not wait and do not re-present the plan."
                            .to_string(),
                    ));
                    iter += 1;
                    continue;
                }
                // Hit max_tokens mid-reply (often mid-checklist, no tool_calls
                // parsed), or returned empty after executing tools without a final summary.
                let truncated = crate::ai::provider::is_output_truncated(
                    &resp.stop_reason,
                    resp.completion_tokens,
                    req.max_tokens,
                );
                let todos_open = tc
                    .session_state
                    .todos(&tc.session_id)
                    .iter()
                    .any(|t| t.status != "completed")
                    || crate::ai::provider::reply_has_open_checklist(&resp.content);
                let pseudo_prompt = crate::ai::provider::reply_has_uncalled_action(&resp.content);
                let had_prior_tool_results =
                    iter > 0 && messages.iter().any(|m| m.role == "tool");
                let empty_post_tool = resp.content.trim().is_empty()
                    && resp.tool_calls.is_empty()
                    && had_prior_tool_results;

                if (truncated || todos_open || pseudo_prompt || empty_post_tool)
                    && truncate_continues < 4
                {
                    truncate_continues += 1;
                    let why = if empty_post_tool {
                        "Model gathered data but returned an empty reply — requesting final summary…"
                            .into()
                    } else if truncated {
                        format!(
                            "Output hit the token cap ({}) — continuing from where it stopped…",
                            resp.completion_tokens.unwrap_or(req.max_tokens)
                        )
                    } else if pseudo_prompt {
                        "Continuing execution — invoking tool for pending check/command…".into()
                    } else {
                        "Checklist still has open steps — continuing.".into()
                    };
                    emit(Some(sink), StreamEvent::Status(why));
                    let nudge = if empty_post_tool {
                        "[system] You executed the tools and gathered the data above, but your reply was empty. \
                         Provide your complete answer and summary to the user now based on what was found. \
                         Do not return an empty response."
                    } else if pseudo_prompt {
                        "[system] You ended with a shell prompt (~#) or an intention to run a check/command, but did not call a tool. Call the required tool (e.g. run_command, read_file) NOW to perform the action. Do not output raw shell prompts in chat."
                    } else {
                        "[system] Your previous reply stopped without finishing. \
                         Continue from exactly where you stopped. Call tools NOW to \
                         complete the remaining checklist steps. Do not restart, do not \
                         repeat finished work, and do not wait for the user."
                    };
                    messages.push(ChatMessage::user(nudge.to_string()));
                    iter += 1;
                    continue;
                }
            }
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
        // Pre-execution tool call auto-repair (Rick-style resilience against malformed calls,
        // markdown file link syntax, string-quoted numbers, and single-item arrays).
        let mut repaired_calls = resp.tool_calls.clone();
        let mut repair_notes: Vec<Option<String>> = Vec::new();
        for call in &mut repaired_calls {
            let note = repair_tool_call(call);
            repair_notes.push(note);
        }

        // Parallelize read-only tool batches (e.g. run_command_all-style multi-host
        // checks issued as separate run_command calls, list/read tools). Mutating tools
        // stay sequential so safety/approvals and ordering stay predictable.
        let all_readonly = repaired_calls
            .iter()
            .all(|c| !tools::tool_is_mutating(&c.name, &c.arguments));
        if all_readonly && repaired_calls.len() > 1 {
            emit(
                Some(sink),
                StreamEvent::Status(format!(
                    "Running {} read-only tools in parallel…",
                    repaired_calls.len()
                )),
            );
            let futs: Vec<_> = repaired_calls
                .into_iter()
                .zip(repair_notes.into_iter())
                .map(|(call, repair_note)| {
                    let telemetry = telemetry.clone();
                    async move {
                        let mut output = tools::dispatch_with_telemetry(tc, &call, sink, Some(&telemetry)).await;
                        if let Some(note) = repair_note {
                            output = format!("<repaired: {note}>\n{output}");
                        }
                        (call, output)
                    }
                })
                .collect();
            let results = futures_util::future::join_all(futs).await;
            for (call, output) in results {
                let capped = cap_tool_result(&call, &output);
                emit(
                    Some(sink),
                    StreamEvent::ToolResult {
                        id: call.id.clone(),
                        output: capped.clone(),
                    },
                );
                messages.push(ChatMessage::tool_result(call.id, capped));
            }
        } else {
            for (call, repair_note) in repaired_calls.into_iter().zip(repair_notes.into_iter()) {
                // The provider already streamed StreamEvent::ToolCall for each call;
                // the single ToolResult is emitted by this loop below. No re-emit here.
                let mut output = tools::dispatch_with_telemetry(tc, &call, sink, Some(&telemetry)).await;
                if let Some(note) = repair_note {
                    output = format!("<repaired: {note}>\n{output}");
                }
                let capped = cap_tool_result(&call, &output);
                emit(
                    Some(sink),
                    StreamEvent::ToolResult {
                        id: call.id.clone(),
                        output: capped.clone(),
                    },
                );
                messages.push(ChatMessage::tool_result(call.id.clone(), capped));
            }
        }

        tc.session_state.store_request_messages(
            &tc.session_id,
            crate::ai::vision::without_images(&messages),
        );

        iter += 1;
    }
    crate::ai::provider::close_unanswered_tool_calls(&mut messages);
    tc.session_state.store_request_messages(
        &tc.session_id,
        crate::ai::vision::without_images(&messages),
    );
    tc.session_state.persist_prefix_cache(&data_dir, &tc.session_id);

    // Learn the *user*, not just the task. Reflection below learns from what went
    // wrong; this reads the user's own standing instructions ("always use k3s",
    // "never touch the db host") out of what they just said and records them in
    // TASTE.md, which rides in the cached system prefix from the next turn on.
    //
    // Without it, a preference only stuck if the model remembered to call taste_save,
    // which it mostly did not — so the same correction was made every week and the
    // agent never appeared to know anyone. Pure pattern matching, no extra model
    // call, so it costs nothing on the turn. Skipped for voice and casual turns,
    // where the phrasing is loose enough to be a false-positive source.
    let learn_user = tc
        .db
        .get_setting("agent.learn_preferences")
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    if learn_user && !conversation && !casual_turn && !last_user_msg.trim().is_empty() {
        let saved = crate::ai::learn::capture_preferences(&tc.home, &last_user_msg);
        for pref in &saved {
            emit(
                Some(sink),
                StreamEvent::Status(format!("Noted your preference: {pref}")),
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
        let lessons = crate::ai::reflection::reflect_and_save_with_targets(
            &tc.home,
            &messages,
            &tc.targets,
            iters_used,
            0,
        );
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
            let outcome = crate::ai::reflection::analyze_turn(&messages, iters_used, 0);
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

    let telemetry = telemetry.snapshot();
    emit(
        Some(sink),
        StreamEvent::TurnTelemetry(crate::ai::provider::TurnTelemetryEvent {
            tool_calls: telemetry.tool_calls,
            tool_cache_lookups: telemetry.tool_cache_lookups,
            tool_cache_hits: telemetry.tool_cache_hits,
            tool_cache_misses: telemetry.tool_cache_misses,
            tool_cache_writes: telemetry.tool_cache_writes,
            tool_cache_hit_rate: telemetry.tool_cache_hit_rate,
        }),
    );
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
        let last_user_pos = messages.iter().rposition(|m| m.role == "user");
        let turn_slice = match last_user_pos {
            Some(idx) => &messages[idx..],
            None => &messages[..],
        };
        let prior_turn_text = turn_slice
            .iter()
            .filter(|m| m.role == "assistant" && !m.content.trim().is_empty())
            .map(|m| m.content.trim())
            .collect::<Vec<_>>()
            .join("\n\n");

        if !prior_turn_text.is_empty() {
            last.content = prior_turn_text;
        } else if ollama_mode {
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

/// Pre-execution sanitizer for tool calls.
/// Fixes markdown-wrapped file links, string-encoded numbers, booleans, and single-item arrays.
/// Returns a description of what was repaired if any modification occurred.
pub fn repair_tool_call(call: &mut crate::ai::provider::ToolCall) -> Option<String> {
    let mut repairs: Vec<String> = Vec::new();

    // 1. Unwrap Markdown file links in string parameters (e.g. path, local_path, remote_path)
    let path_keys = [
        "path",
        "local_path",
        "remote_path",
        "source",
        "glob",
        "backup_dir",
        "key_path",
        "target_dir",
    ];
    for key in &path_keys {
        if let Some(val) = call.arguments.get_mut(*key) {
            if let Some(s) = val.as_str() {
                if let Some(unwrapped) = unwrap_markdown_path(s) {
                    *val = json!(unwrapped);
                    repairs.push(format!("unwrapped markdown link in '{key}'"));
                }
            }
        }
    }

    // 2. Coerce string numbers into JSON integers
    let int_keys = [
        "offset",
        "limit",
        "port",
        "head_limit",
        "delay_secs",
        "max_cycles",
        "max_tokens",
        "width",
        "height",
    ];
    for key in &int_keys {
        if let Some(val) = call.arguments.get_mut(*key) {
            if let Some(s) = val.as_str() {
                if let Ok(num) = s.parse::<i64>() {
                    *val = json!(num);
                    repairs.push(format!("coerced '{key}' string to integer ({num})"));
                }
            }
        }
    }

    // 3. Coerce boolean strings into JSON booleans
    let bool_keys = [
        "case_insensitive",
        "replace_all",
        "submit",
        "multi",
        "save_backup",
        "plan_mode",
    ];
    for key in &bool_keys {
        if let Some(val) = call.arguments.get_mut(*key) {
            if let Some(s) = val.as_str() {
                match s.trim().to_lowercase().as_str() {
                    "true" | "yes" | "1" => {
                        *val = json!(true);
                        repairs.push(format!("coerced '{key}' string to boolean (true)"));
                    }
                    "false" | "no" | "0" => {
                        *val = json!(false);
                        repairs.push(format!("coerced '{key}' string to boolean (false)"));
                    }
                    _ => {}
                }
            }
        }
    }

    // 4. Wrap single string/object into array if schema expects an array
    let array_keys = [
        "todos",
        "files",
        "questions",
        "success_criteria",
        "check_tooling",
        "hard_constraints",
        "options",
    ];
    for key in &array_keys {
        if let Some(val) = call.arguments.get_mut(*key) {
            if val.is_string() || val.is_object() {
                let single = val.clone();
                *val = json!([single]);
                repairs.push(format!("wrapped single '{key}' item into an array"));
            }
        }
    }

    if repairs.is_empty() {
        None
    } else {
        Some(repairs.join(", "))
    }
}

fn unwrap_markdown_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    // [filename](file:///path) or [filename](path)
    if trimmed.starts_with('[') && trimmed.contains("](") && trimmed.ends_with(')') {
        let parts: Vec<&str> = trimmed.split("](").collect();
        if parts.len() == 2 {
            let mut target = parts[1].trim_end_matches(')').trim();
            if let Some(stripped) = target.strip_prefix("file:///") {
                target = stripped;
            } else if let Some(stripped) = target.strip_prefix("file://") {
                target = stripped;
            }
            return Some(target.to_string());
        }
    }
    // `file:///path` or `file://path` or `path`
    if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 2 {
        let inside = trimmed[1..trimmed.len() - 1].trim();
        let target = inside
            .strip_prefix("file:///")
            .or_else(|| inside.strip_prefix("file://"))
            .unwrap_or(inside);
        return Some(target.to_string());
    }
    if let Some(target) = trimmed.strip_prefix("file:///") {
        return Some(target.to_string());
    }
    if let Some(target) = trimmed.strip_prefix("file://") {
        return Some(target.to_string());
    }
    None
}

/// Send a lightweight 1-token probe to pre-warm the prompt cache prefix before a heavy turn
/// or scheduled task (adaptive eviction-gap warming).
#[allow(dead_code)]
pub async fn warm_cache_prefix(tc: &ToolContext, prompt_hint: Option<&str>) -> Result<(), String> {
    let resolved = match registry::build(
        &tc.db,
        &tc.db
            .get_setting("agent.provider")
            .ok()
            .flatten()
            .unwrap_or_else(|| "openai".into()),
    ) {
        Ok(r) => r,
        Err(e) => return Err(e),
    };
    let mut req = ChatRequest::new(&resolved.model);
    req.max_tokens = 1;
    req.session_id = tc.session_id.clone();
    req.system = prompt_hint.unwrap_or("System ready.").to_string();
    req.messages = vec![ChatMessage::user("ping")];
    let _ = resolved.provider.chat(&req, None).await;
    Ok(())
}

