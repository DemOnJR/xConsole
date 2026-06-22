//! System-prompt assembly, mirroring Hermes' three-tier design
//! (`agent/system_prompt.py`): a `stable` identity/guidance tier, a `context`
//! tier (project files), and a `volatile` tier (memory + runtime facts). The
//! tiers are joined with blank lines. Built once per turn here; callers cache it
//! across a session and only rebuild after compression.

use chrono::Local;

use crate::ai::provider::ChatMessage;
use crate::ai::{memory, skills, soul, AgentHome};
use crate::storage::Db;

/// Keep at most this many recent messages in the live window. Older turns are
/// dropped and replaced by a short synthetic note (a lightweight stand-in for a
/// full LLM-summarizing compressor; the durable facts live in MEMORY.md).
pub const MAX_WINDOW_MESSAGES: usize = 40;

/// Trim an over-long message history to the recent window. Tool-result messages
/// are never separated from the assistant tool call they answer, because the
/// window only ever drops from the front in role-agnostic order and the most
/// recent turns (which are kept) remain internally consistent.
pub fn compress_window(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    if messages.len() <= MAX_WINDOW_MESSAGES {
        return messages;
    }
    let dropped = messages.len() - MAX_WINDOW_MESSAGES;
    let mut out = Vec::with_capacity(MAX_WINDOW_MESSAGES + 1);
    out.push(ChatMessage::user(format!(
        "[Earlier conversation compressed: {dropped} older messages omitted. \
Durable facts were saved to memory.]"
    )));
    out.extend(messages.into_iter().skip(dropped));
    out
}

/// Inputs needed to assemble the prompt for a turn.
pub struct PromptContext<'a> {
    pub home: &'a AgentHome,
    pub db: &'a Db,
    pub model_label: &'a str,
    pub provider_label: &'a str,
    /// Resolved safety mode ("full" | "approve" | "allowlist").
    pub safety: &'a str,
    /// Number of VPS targets the agent may act on this turn.
    pub target_count: usize,
    /// Compact summary of the current conversation thread (Hermes-style).
    pub conversation_summary: Option<String>,
    /// Whether tool use is available this turn.
    pub has_tools: bool,
    /// Local Ollama: only VPS tools are registered (no terraform/cloud schemas).
    pub vps_tools_only: bool,
    /// Local Ollama context window — used to trim prompt tiers when space is tight.
    pub ollama_num_ctx: Option<u32>,
    /// Selected VPS ids for this turn (exact values for run_command).
    pub target_ids: &'a [String],
    /// Greeting / small talk — do not pitch server checks.
    pub casual_turn: bool,
    /// When user says "both/all" but selection differs — injected into volatile tier.
    pub target_selection_note: Option<String>,
    /// Ponytail-minimal tiers when context is tight (Hermes auto-compact).
    pub force_minimal_prompt: bool,
}

/// Guidance injected when the agent has command/file tools available.
const TOOL_GUIDANCE: &str = "You can act on the user's servers through your tools. \
Prefer running a real command/tool over describing what you would do. Inspect \
before you change, make minimal reversible edits, and verify the result. \
For infrastructure, load skills meta/ponytail and the matching infra/terraform-* skill first, \
then use project_*, cloud_*, tfc_*, and terraform_* tools. When a task is complete, stop.";

const VPS_TOOL_GUIDANCE: &str = "You can act on the user's VPS targets through your tools. \
When the user asks about both/all/each server, use run_command_all (one call covers every selected target). \
Live SSH commands may already have run — see snapshot and live command sections below. \
Summarize that output directly; NEVER say you will run commands or ask to confirm read-only checks. \
For uptime/reboot: use the INTERPRETATION line (e.g. '20:59' = ~21 hours) — never invent calendar dates. \
For write_file on Linux VPS as root: use /root/ or /tmp/ paths (e.g. /root/hello.py) — never /home/root/. \
Use underscores in filenames (hello.py not hello world.py) unless the user asked for spaces. \
Do not SSH or write files when the user only asked for example code in chat — answer in the message instead. \
When a task is complete, stop.";

const WEB_GUIDANCE: &str = "You have internet access via web_search and web_fetch. Use them for current \
facts (weather, news, prices, live docs) — never claim you cannot access the web. \
For weather: web_search the city, or web_fetch https://wttr.in/City?format=3 (URL-encode the city name). \
Prefer a tool call over guessing when the answer depends on real-time data.";

const CASUAL_GUIDANCE: &str = "The user sent a greeting or casual message. Reply briefly and naturally. \
Do not mention VPS, servers, RAM, disk, or offer infrastructure checks unless they asked.";

const PONYTAIL_COMPACT_GUIDANCE: &str = "Context was auto-compacted (ponytail mode). Use the smallest \
correct action: one targeted command when possible, minimal prose, no redundant health checks. \
Stop at the first rung on the ponytail ladder — YAGNI, stdlib/native before dependencies.";

fn is_minimal_prompt(ctx: &PromptContext) -> bool {
    ctx.force_minimal_prompt
        || (ctx.vps_tools_only
            && ctx
                .ollama_num_ctx
                .is_some_and(|n| n < OLLAMA_COMPACT_CTX))
}

/// Context sizes below this use a trimmed prompt (no infra inventory, no skill index).
const OLLAMA_COMPACT_CTX: u32 = 65_536;

/// Guidance for the built-in memory tool.
const MEMORY_GUIDANCE: &str = "You have a persistent memory. Save durable, \
reusable facts (server roles, conventions, credentials locations, recurring \
fixes) with the memory tool; keep entries terse. Do not store secrets verbatim.";

fn safety_guidance(safety: &str) -> &'static str {
    match safety {
        "full" => "Safety mode: FULL AUTONOMY. You may run any command without \
asking, but remain careful with destructive operations.",
        "allowlist" => "Safety mode: ALLOWLIST. Read-only/safe commands run \
automatically; destructive or unknown commands require user approval before \
execution.",
        _ => "Safety mode: APPROVE. Every command you run must be approved by the \
user first; propose precise commands and wait.",
    }
}

/// Assemble the full system prompt for a turn.
pub fn build_system_prompt(ctx: &PromptContext) -> String {
    let (tiers, _) = collect_prompt_tiers(ctx);
    join_tiers(tiers)
}

/// Per-tier strings for context-usage reporting (same logic as `build_system_prompt`).
pub struct PromptParts {
    pub rules_tokens: u32,
    pub skills_tokens: u32,
    pub memory_tokens: u32,
    pub infra_tokens: u32,
    pub summary_tokens: u32,
}

pub fn measure_prompt_parts(ctx: &PromptContext) -> PromptParts {
    let minimal = is_minimal_prompt(ctx);

    let soul = if ctx.casual_turn && ctx.vps_tools_only {
        CASUAL_GUIDANCE.to_string()
    } else {
        soul::load(ctx.home)
    };

    let mut rules = vec![soul];
    if ctx.has_tools {
        rules.push(if ctx.vps_tools_only {
            VPS_TOOL_GUIDANCE.to_string()
        } else {
            TOOL_GUIDANCE.to_string()
        });
        rules.push(WEB_GUIDANCE.to_string());
        if !minimal {
            rules.push(MEMORY_GUIDANCE.to_string());
        }
        rules.push(safety_guidance(ctx.safety).to_string());
    }
    if ctx.force_minimal_prompt {
        rules.push(PONYTAIL_COMPACT_GUIDANCE.to_string());
    }
    if let Some(note) = &ctx.target_selection_note {
        if !note.trim().is_empty() {
            rules.push(note.trim().to_string());
        }
    }

    let skills_text = if !minimal {
        if ctx.force_minimal_prompt {
            skills::system_index_minimal(ctx.home)
        } else {
            skills::system_index(ctx.home)
        }
    } else {
        String::new()
    };

    let mut infra_parts: Vec<String> = Vec::new();
    if !ctx.casual_turn && !ctx.target_ids.is_empty() {
        let catalog = crate::ai::tools::format_targets_catalog(ctx.db, ctx.target_ids);
        if !catalog.is_empty() {
            infra_parts.push(catalog);
        }
    }
    if !minimal {
        let infra = crate::infra::summary::format_infra_summary(ctx.db);
        if !infra.is_empty() {
            infra_parts.push(infra);
        }
    }

    let mem = if !minimal {
        memory::format_for_prompt(ctx.home)
    } else {
        String::new()
    };

    let summary = ctx
        .conversation_summary
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("# This conversation (compact thread context)\n{}", s.trim()))
        .unwrap_or_default();

    PromptParts {
        rules_tokens: estimate_tokens_from_chars(rules.join("\n\n").len()),
        skills_tokens: estimate_tokens_from_chars(skills_text.len()),
        memory_tokens: estimate_tokens_from_chars(mem.len()),
        infra_tokens: estimate_tokens_from_chars(infra_parts.join("\n\n").len()),
        summary_tokens: estimate_tokens_from_chars(summary.len()),
    }
}

fn estimate_tokens_from_chars(chars: usize) -> u32 {
    if chars == 0 {
        0
    } else {
        ((chars as f64) / 4.0).ceil() as u32
    }
}

fn join_tiers(tiers: [Vec<String>; 3]) -> String {
    tiers
        .into_iter()
        .map(|tier| {
            tier.into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_prompt_tiers(ctx: &PromptContext) -> ([Vec<String>; 3], bool) {
    let minimal = is_minimal_prompt(ctx);

    let mut stable: Vec<String> = Vec::new();
    if ctx.casual_turn && ctx.vps_tools_only {
        stable.push(CASUAL_GUIDANCE.to_string());
    } else {
        stable.push(soul::load(ctx.home));
    }

    if ctx.has_tools {
        stable.push(if ctx.vps_tools_only {
            VPS_TOOL_GUIDANCE.to_string()
        } else {
            TOOL_GUIDANCE.to_string()
        });
        stable.push(WEB_GUIDANCE.to_string());
        if !minimal {
            stable.push(MEMORY_GUIDANCE.to_string());
        }
        stable.push(safety_guidance(ctx.safety).to_string());
    }
    if ctx.force_minimal_prompt {
        stable.push(PONYTAIL_COMPACT_GUIDANCE.to_string());
    }

    if !minimal {
        let skills_index = if ctx.force_minimal_prompt {
            skills::system_index_minimal(ctx.home)
        } else {
            skills::system_index(ctx.home)
        };
        if !skills_index.is_empty() {
            stable.push(skills_index);
        }
    }

    let mut context: Vec<String> = Vec::new();
    if !ctx.casual_turn && !ctx.target_ids.is_empty() {
        let catalog = crate::ai::tools::format_targets_catalog(ctx.db, ctx.target_ids);
        if !catalog.is_empty() {
            context.push(catalog);
        }
    }
    if !minimal {
        let infra = crate::infra::summary::format_infra_summary(ctx.db);
        if !infra.is_empty() {
            context.push(infra);
        }
    }

    let mut volatile: Vec<String> = Vec::new();
    if let Some(note) = &ctx.target_selection_note {
        if !note.trim().is_empty() {
            volatile.push(note.trim().to_string());
        }
    }
    if let Some(summary) = &ctx.conversation_summary {
        if !summary.trim().is_empty() {
            volatile.push(format!(
                "# This conversation (compact thread context)\n{}",
                summary.trim()
            ));
        }
    }
    let mem = memory::format_for_prompt(ctx.home);
    if !mem.is_empty() && !minimal {
        volatile.push(mem);
    }
    let mut runtime = format!("Date: {}", Local::now().format("%A, %B %d, %Y"));
    if !ctx.model_label.is_empty() {
        runtime.push_str(&format!("\nModel: {}", ctx.model_label));
    }
    if !ctx.provider_label.is_empty() {
        runtime.push_str(&format!("\nProvider: {}", ctx.provider_label));
    }
    if !ctx.casual_turn {
        runtime.push_str(&format!(
            "\nReachable VPS targets this session: {}",
            ctx.target_count
        ));
    }
    if ctx.target_count == 0 {
        runtime.push_str(if ctx.vps_tools_only {
            "\nNo VPS targets selected: SSH tools unavailable this turn."
        } else {
            "\nNo VPS targets selected: SSH tools unavailable; use project_*, cloud_*, tfc_*, terraform_* for infra."
        });
    }
    volatile.push(runtime);

    ([stable, context, volatile], minimal)
}
