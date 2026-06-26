//! Headless benchmark / eval harness for the agent + local-model path.
//!
//! Invoked as a separate binary (`xconsole-bench.exe`, see `src/bin/xconsole-bench.rs`)
//! so it links the same `xconsole_lib` code the desktop app runs but never starts a
//! webview — and a running xConsole instance can't lock its output exe.
//!
//! It exercises the REAL model-facing path: the same three-tier system prompt
//! (`context::build_system_prompt`), the same curated Ollama tool schema
//! (`tools::definitions_for_ollama`), and the same `Provider::chat` implementation
//! the app uses — against a local Ollama model. So latency and tool-selection
//! numbers reflect production behavior, not a stub.
//!
//! Usage:
//!   xconsole-bench agent    [--model qwen3.5:9b] [--base http://localhost:11434] [--ctx 65536] [--out results.json]
//!   xconsole-bench ablation [--model ...] [--samples N]   # soul/memory/skills/brief cost vs quality
//!   xconsole-bench llm      [--model ...] [--ctx ...]
//!   xconsole-bench all
//!   xconsole-bench hooks    [--out results.json]   # hooks dispatch overhead (no model)
//!   xconsole-bench selftest                        # pure-logic + live-hook checks (no model)
//!
//! These are REGRESSION benchmarks: run them, change a feature, run them again,
//! and compare the JSON to see whether latency / pass-rate improved.

use std::path::PathBuf;
use std::time::Instant;

use serde_json::{json, Value};

use crate::ai::context::{self, PromptContext};
use crate::ai::provider::{ChatMessage, ChatRequest, Provider, StreamEvent, StreamStats, ToolDef};
use crate::ai::registry::{self, ResolvedProvider};
use crate::ai::{skills, soul, tools, AgentHome};
use crate::storage::models::AiProviderInput;
use crate::storage::Db;

const DEFAULT_MODEL: &str = "qwen3.5:9b";
const DEFAULT_BASE: &str = "http://localhost:11434";
const DEFAULT_CTX: u32 = 65_536;

/// Entry point from the thin bin. Builds a Tokio runtime and runs the requested mode.
pub fn run(args: &[String]) {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("bench: failed to start tokio runtime: {e}");
            std::process::exit(1);
        }
    };
    let code = rt.block_on(run_async(args));
    std::process::exit(code);
}

fn arg_val(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

async fn run_async(args: &[String]) -> i32 {
    // Mode is the first positional arg (not a --flag); default to the agent eval.
    let mode = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "agent".to_string());
    let model = arg_val(args, "--model").unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let base = arg_val(args, "--base").unwrap_or_else(|| DEFAULT_BASE.to_string());
    let num_ctx: u32 = arg_val(args, "--ctx")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CTX);
    let samples: usize = arg_val(args, "--samples")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let out = arg_val(args, "--out");

    println!("xConsole bench — mode={mode} model={model} base={base} num_ctx={num_ctx}");

    // Pure-logic self-tests (reflection, voice prompt, hooks) — no Ollama needed.
    if mode == "selftest" {
        let mut code = selftest();
        if selftest_hooks_live().await != 0 {
            code = 1;
        }
        return code;
    }

    // Hooks overhead benchmark — needs no model, so run before the Ollama preflight.
    if mode == "hooks" {
        return bench_hooks(out).await;
    }

    // Preflight: Ollama up and the model present?
    match preflight(&base, &model).await {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\nbench: {e}");
            eprintln!("Start Ollama with `ollama serve` and `ollama pull {model}`, then retry.");
            return 2;
        }
    }

    let env = match BenchEnv::setup(&model, &base, num_ctx, samples) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("bench: setup failed: {e}");
            return 1;
        }
    };

    let report = match mode.as_str() {
        "llm" => bench_llm(&env).await,
        "agent" => bench_agent(&env).await,
        "ablation" => bench_ablation(&env).await,
        "all" => {
            let mut a = bench_llm(&env).await;
            let b = bench_agent(&env).await;
            merge_reports(&mut a, b);
            a
        }
        other => {
            eprintln!(
                "bench: unknown mode '{other}' (use: agent | ablation | llm | all | hooks | selftest)"
            );
            return 1;
        }
    };

    if let Some(path) = out {
        match std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default()) {
            Ok(()) => println!("\nWrote results → {path}"),
            Err(e) => eprintln!("bench: could not write {path}: {e}"),
        }
    }
    env.cleanup();
    0
}

/// A temp DB + agent home + a resolved Ollama provider, mirroring what `ai_chat`
/// builds — minus the AppHandle (none of the model-facing path needs it).
struct BenchEnv {
    db: Db,
    home: AgentHome,
    provider_id: String,
    model: String,
    num_ctx: u32,
    /// Samples per scenario (>1 smooths out Ollama's non-deterministic sampling).
    samples: usize,
    root: PathBuf,
}

impl BenchEnv {
    fn setup(model: &str, base: &str, num_ctx: u32, samples: usize) -> Result<Self, String> {
        let root = std::env::temp_dir().join(format!("xconsole-bench-{}", std::process::id()));
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let db = Db::open(&root.join("bench.db")).map_err(|e| e.to_string())?;
        let home = AgentHome::new(root.join("agent"));
        skills::seed_defaults(&home);

        let extra = json!({ "num_ctx": num_ctx, "think": false }).to_string();
        let prov = db
            .upsert_provider(&AiProviderInput {
                id: None,
                name: "bench-ollama".into(),
                kind: "ollama".into(),
                model: Some(model.to_string()),
                base_url: Some(base.to_string()),
                bin_path: None,
                extra_json: Some(extra),
                enabled: true,
                secret: None,
            })
            .map_err(|e| e.to_string())?;
        db.set_setting("agent.active_provider", &prov.id).ok();
        db.set_setting("agent.safety_mode", "full").ok();

        Ok(Self {
            db,
            home,
            provider_id: prov.id,
            model: model.to_string(),
            num_ctx,
            samples: samples.max(1),
            root,
        })
    }

    fn resolve(&self) -> Result<ResolvedProvider, String> {
        registry::build(&self.db, &self.provider_id)
    }

    /// Build the real system prompt + Ollama tool schema for a scenario. In
    /// `conversation` (voice) mode a pure chat turn (no targets) carries no tools.
    fn build_prompt(
        &self,
        targets: &[String],
        casual: bool,
        conversation: bool,
    ) -> (String, Vec<ToolDef>) {
        // Voice keeps the curated tool set (web + local/VPS); only the prose is trimmed.
        let _ = conversation;
        let tool_defs = tools::definitions_for_ollama(&self.home, targets.len(), casual);
        let ctx = PromptContext {
            home: &self.home,
            db: &self.db,
            model_label: &self.model,
            provider_label: "bench (Ollama local)",
            safety: "full",
            target_count: targets.len(),
            conversation_summary: None,
            has_tools: !tool_defs.is_empty(),
            vps_tools_only: true,
            ollama_num_ctx: Some(self.num_ctx),
            target_ids: targets,
            casual_turn: casual,
            target_selection_note: None,
            force_minimal_prompt: false,
            plan_mode: false,
            workspace_context: None,
            canvas_context: None,
            conversation,
        };
        (context::build_system_prompt(&ctx), tool_defs)
    }

    /// Build the prompt against an arbitrary agent home + optional workspace brief
    /// block — used by the ablation to seed each tier (soul/memory/skills) into a
    /// dedicated home and toggle the project brief via `workspace_context`.
    fn build_prompt_with(
        &self,
        home: &AgentHome,
        workspace_context: Option<String>,
        targets: &[String],
        casual: bool,
    ) -> (String, Vec<ToolDef>) {
        let tool_defs = tools::definitions_for_ollama(home, targets.len(), casual);
        let ctx = PromptContext {
            home,
            db: &self.db,
            model_label: &self.model,
            provider_label: "bench (Ollama local)",
            safety: "full",
            target_count: targets.len(),
            conversation_summary: None,
            has_tools: !tool_defs.is_empty(),
            vps_tools_only: true,
            ollama_num_ctx: Some(self.num_ctx),
            target_ids: targets,
            casual_turn: casual,
            target_selection_note: None,
            force_minimal_prompt: false,
            plan_mode: false,
            workspace_context,
            canvas_context: None,
            conversation: false,
        };
        (context::build_system_prompt(&ctx), tool_defs)
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Result of a single model turn, with client-measured latency.
struct TurnResult {
    content: String,
    tool_calls: Vec<String>,
    ttft_ms: u128,
    total_ms: u128,
    gen_tps: f32,
    prompt_tokens: u32,
    completion_tokens: u32,
    error: Option<String>,
}

async fn one_turn(
    provider: &dyn Provider,
    model: &str,
    system: String,
    tool_defs: Vec<ToolDef>,
    user: &str,
) -> TurnResult {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut req = ChatRequest::new(model);
    req.system = system;
    req.messages = vec![ChatMessage::user(user)];
    req.tools = tool_defs;

    let t0 = Instant::now();
    let drain = tokio::spawn(async move {
        let mut ttft: Option<u128> = None;
        let mut stats: Option<StreamStats> = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                StreamEvent::Text(t) if ttft.is_none() && !t.is_empty() => {
                    ttft = Some(t0.elapsed().as_millis());
                }
                StreamEvent::Stats(s) => stats = Some(s),
                _ => {}
            }
        }
        (ttft, stats)
    });

    let resp = provider.chat(&req, Some(&tx)).await;
    drop(tx);
    let (ttft, stats) = drain.await.unwrap_or((None, None));
    let total_ms = t0.elapsed().as_millis();

    match resp {
        Ok(r) => {
            let s = stats.unwrap_or(StreamStats {
                completion_tokens: 0,
                prompt_tokens: None,
                duration_ms: 0,
                tokens_per_sec: 0.0,
            });
            TurnResult {
                content: r.content,
                tool_calls: r.tool_calls.into_iter().map(|c| c.name).collect(),
                ttft_ms: ttft.unwrap_or(total_ms),
                total_ms,
                gen_tps: s.tokens_per_sec,
                prompt_tokens: s.prompt_tokens.unwrap_or(0),
                completion_tokens: s.completion_tokens,
                error: None,
            }
        }
        Err(e) => TurnResult {
            content: String::new(),
            tool_calls: vec![],
            ttft_ms: ttft.unwrap_or(total_ms),
            total_ms,
            gen_tps: 0.0,
            prompt_tokens: 0,
            completion_tokens: 0,
            error: Some(e),
        },
    }
}

// ---- Agent eval ----------------------------------------------------------

enum Expect {
    /// A plain answer with no tool call (casual chat, in-message code, math).
    NoTools,
    /// One of these tool names should be selected.
    ToolOneOf(&'static [&'static str]),
    /// A no-tool answer that must contain this (case-insensitive) substring.
    Contains(&'static str),
    /// A no-tool answer that must contain at least one of these substrings.
    ContainsAny(&'static [&'static str]),
}

struct Scenario {
    name: &'static str,
    user: &'static str,
    targets: usize,
    casual: bool,
    /// Build the lightweight spoken-voice prompt for this scenario.
    conversation: bool,
    expect: Expect,
}

/// The eval set. Each scenario asserts a behavior that a useful agent must get
/// right; the pass-rate is a quality signal we track across changes.
fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "casual-greeting",
            user: "hey there, how's it going?",
            targets: 0,
            casual: true,
            conversation: false,
            expect: Expect::NoTools,
        },
        Scenario {
            name: "math-no-tools",
            user: "What is 17 * 23? Just the number.",
            targets: 0,
            casual: false,
            conversation: false,
            expect: Expect::Contains("391"),
        },
        Scenario {
            name: "voice-terse-explain",
            user: "In one short sentence, what is SSH?",
            targets: 0,
            casual: false,
            conversation: false,
            expect: Expect::NoTools,
        },
        Scenario {
            name: "single-server-command",
            user: "Show me the disk usage on my server.",
            targets: 1,
            casual: false,
            conversation: false,
            // list_vps_targets is a legitimate investigative first step before acting.
            expect: Expect::ToolOneOf(&["run_command", "run_command_all", "list_vps_targets"]),
        },
        Scenario {
            name: "all-servers-command",
            user: "Check uptime on all of my servers.",
            targets: 2,
            casual: false,
            conversation: false,
            expect: Expect::ToolOneOf(&["run_command_all", "run_command", "list_vps_targets"]),
        },
        Scenario {
            name: "write-remote-file",
            user: "Create a file /root/hello.py on my server that prints hello world.",
            targets: 1,
            casual: false,
            conversation: false,
            expect: Expect::ToolOneOf(&["write_file"]),
        },
        Scenario {
            name: "in-chat-code-only",
            user: "Just show me, in chat, a bash one-liner to count lines in a file. Don't run anything.",
            targets: 1,
            casual: false,
            conversation: false,
            expect: Expect::NoTools,
        },
        // --- Voice (conversation-mode) scenarios: same asks, lightweight prompt ---
        Scenario {
            name: "voice:greeting",
            user: "hey, what's up?",
            targets: 0,
            casual: true,
            conversation: true,
            expect: Expect::NoTools,
        },
        Scenario {
            name: "voice:explain",
            user: "What is a reverse proxy?",
            targets: 0,
            casual: false,
            conversation: true,
            expect: Expect::NoTools,
        },
        Scenario {
            // Regression guard: a spoken turn with no VPS must still reach the web.
            name: "voice:weather",
            user: "What's the weather in Tor San Lorenzo, Italy right now?",
            targets: 0,
            casual: false,
            conversation: true,
            expect: Expect::ToolOneOf(&["web_search", "web_fetch", "geo_locate"]),
        },
        Scenario {
            name: "voice:server-action",
            user: "Restart nginx on my server please.",
            targets: 1,
            casual: false,
            conversation: true,
            expect: Expect::ToolOneOf(&["run_command", "run_command_all", "list_vps_targets"]),
        },
    ]
}

fn score(expect: &Expect, r: &TurnResult) -> bool {
    if r.error.is_some() {
        return false;
    }
    match expect {
        Expect::NoTools => r.tool_calls.is_empty(),
        Expect::ToolOneOf(names) => r.tool_calls.iter().any(|n| names.contains(&n.as_str())),
        Expect::Contains(s) => {
            r.tool_calls.is_empty() && r.content.to_lowercase().contains(&s.to_lowercase())
        }
        Expect::ContainsAny(subs) => {
            let lc = r.content.to_lowercase();
            r.tool_calls.is_empty() && subs.iter().any(|s| lc.contains(&s.to_lowercase()))
        }
    }
}

async fn bench_agent(env: &BenchEnv) -> Value {
    let resolved = match env.resolve() {
        Ok(r) => r,
        Err(e) => return json!({ "mode": "agent", "error": e }),
    };
    // Warm the model into VRAM so per-scenario latencies reflect steady state, not a
    // one-off cold load (keeps the baseline comparable across runs).
    println!("\n(warming model…)");
    let (warm_sys, _) = env.build_prompt(&[], true, false);
    let _ = one_turn(resolved.provider.as_ref(), &env.model, warm_sys, vec![], "hi").await;

    println!(
        "\n=== AGENT EVAL ({} scenarios × {} sample(s)) ===",
        scenarios().len(),
        env.samples
    );
    println!(
        "{:<24} {:>6} {:>8} {:>8} {:>7} {:>6}  {}",
        "scenario", "pass", "ttft_ms", "total_ms", "gen_t/s", "ptok", "selected"
    );

    let mut results = Vec::new();
    let mut passes = 0usize; // scenarios passing by majority of samples
    let mut total_ms_sum = 0u128;
    let mut total_turns = 0u128;
    let scns = scenarios();
    for s in &scns {
        let targets: Vec<String> = (0..s.targets).map(|i| format!("vps-{i}")).collect();
        let mut k = 0usize;
        let mut ttft_sum = 0u128;
        let mut total_sum = 0u128;
        let mut gen_tps = 0.0f32;
        let mut ptok = 0u32;
        let mut last_selected = String::new();
        for _ in 0..env.samples {
            let (system, tool_defs) = env.build_prompt(&targets, s.casual, s.conversation);
            let r =
                one_turn(resolved.provider.as_ref(), &env.model, system, tool_defs, s.user).await;
            if score(&s.expect, &r) {
                k += 1;
            }
            ttft_sum += r.ttft_ms;
            total_sum += r.total_ms;
            gen_tps = r.gen_tps;
            ptok = r.prompt_tokens;
            last_selected = if r.tool_calls.is_empty() {
                r.error
                    .as_ref()
                    .map(|e| format!("ERROR: {}", e.chars().take(36).collect::<String>()))
                    .unwrap_or_else(|| "(text)".to_string())
            } else {
                r.tool_calls.join(",")
            };
        }
        let n = env.samples as u128;
        let ttft_avg = ttft_sum / n;
        let total_avg = total_sum / n;
        // Pass the scenario when a strict majority of samples passed.
        let ok = k * 2 > env.samples;
        if ok {
            passes += 1;
        }
        total_ms_sum += total_sum;
        total_turns += n;
        println!(
            "{:<24} {:>6} {:>8} {:>8} {:>7.1} {:>6}  {}",
            s.name,
            format!("{k}/{}", env.samples),
            ttft_avg,
            total_avg,
            gen_tps,
            ptok,
            last_selected
        );
        results.push(json!({
            "scenario": s.name,
            "pass": ok,
            "passed_samples": k,
            "samples": env.samples,
            "ttft_ms_avg": ttft_avg,
            "total_ms_avg": total_avg,
            "gen_tps": gen_tps,
            "prompt_tokens": ptok,
            "last_selected": last_selected,
        }));
    }
    let n = scns.len().max(1);
    println!(
        "\nPASS {passes}/{} scenarios ({:.0}%)   avg turn {} ms over {} turns",
        scns.len(),
        100.0 * passes as f32 / n as f32,
        if total_turns > 0 { total_ms_sum / total_turns } else { 0 },
        total_turns
    );

    json!({
        "mode": "agent",
        "model": env.model,
        "num_ctx": env.num_ctx,
        "samples": env.samples,
        "pass": passes,
        "total": scns.len(),
        "avg_turn_ms": if total_turns > 0 { total_ms_sum / total_turns } else { 0 },
        "scenarios": results,
    })
}

// ---- Ablation: cost vs. quality of each prompt system --------------------
//
// Measures what the four "agent-brain" systems — SOUL (identity), MEMORY
// (MEMORY.md + USER.md), SKILLS (the skills index), and the PROJECT BRIEF (the
// per-workspace CONTEXT.md the agent keeps updated) — cost in prompt tokens /
// latency and what they buy in answer quality, by toggling each one off in turn
// and re-running the same scenarios on the real production prompt assembly.

/// One ablation configuration: which of the four systems are present.
struct Variant {
    name: &'static str,
    soul: bool,
    memory: bool,
    skills: bool,
    brief: bool,
}

fn ablation_variants() -> Vec<Variant> {
    vec![
        Variant { name: "full",    soul: true,  memory: true,  skills: true,  brief: true },
        Variant { name: "-soul",   soul: false, memory: true,  skills: true,  brief: true },
        Variant { name: "-memory", soul: true,  memory: false, skills: true,  brief: true },
        Variant { name: "-skills", soul: true,  memory: true,  skills: false, brief: true },
        Variant { name: "-brief",  soul: true,  memory: true,  skills: true,  brief: false },
        Variant { name: "bare",    soul: false, memory: false, skills: false, brief: false },
    ]
}

// Realistic seed content representative of the user's real uses: coding,
// VPS/server management, and a personal agent. The ablation removes one block at
// a time so the measured deltas reflect the cost/benefit of THAT system.
const ABL_MEMORY: &str = "\
- The user's primary VPS `web-1` runs Ubuntu 22.04 with nginx + a Node.js app under pm2; deploy with `pm2 restart shopfront`.
- The database server `db-1` runs PostgreSQL 16; never run destructive SQL without a `pg_dump` backup first.
- [lesson] When `apt` fails with a dpkg lock error, wait and retry — do NOT kill dpkg; an alternative is to check `/var/lib/dpkg/lock`.
- Code style: the user's projects use TypeScript strict mode and pnpm. Always use pnpm, never npm.
- The user prefers concise, direct answers with no filler.";

const ABL_USER: &str = "\
# About the user
- Solo developer running a few personal VPS servers and side projects.
- Uses xConsole for coding, managing VPS servers, and as a general personal agent.
- Hardware: Ryzen 9 5900X, 32 GB RAM, RX 9060 XT; runs local models via Ollama.
- Comfortable in the terminal; wants no-fluff answers.";

/// The per-workspace project brief block, in the exact shape
/// `workspace_context::build_workspace_block` produces for the prompt's context tier.
fn ablation_brief_block() -> String {
    "# Active workspace: shopfront\n\
This is the project the user is working in. Use this context; keep the brief current \
with set_project_brief; save durable project facts with the memory tool.\n\n\
## Project brief\n\
Purpose: deploy and operate the \"shopfront\" Node.js web app on web-1.\n\
Stack: Node 20, Express, PostgreSQL (db-1), nginx reverse proxy, pm2.\n\
Important paths: /srv/shopfront (app), /etc/nginx/sites-enabled/shopfront.\n\
Run/build/test: `pnpm install`, `pnpm build`, `pnpm test`.\n\
Deploy: `pm2 restart shopfront`.\n\
Conventions: TypeScript strict, conventional commits, never edit on prod without a backup."
        .to_string()
}

/// Seed a dedicated agent home for a variant (soul / memory / skills toggled via
/// on-disk content, exactly as production reads them). Returns the home plus the
/// optional brief block to pass as `workspace_context`.
fn seed_variant_home(root: &std::path::Path, v: &Variant) -> (AgentHome, Option<String>) {
    let dir = root.join(format!("abl-{}", v.name.trim_start_matches('-')));
    let _ = std::fs::remove_dir_all(&dir);
    let home = AgentHome::new(dir);
    // SOUL.md: realistic identity when on; explicitly emptied when off.
    let _ = std::fs::write(home.soul(), if v.soul { soul::DEFAULT_SOUL_MD } else { "" });
    // MEMORY.md + USER.md: written only when memory is on.
    if v.memory {
        let _ = std::fs::write(home.memory(), ABL_MEMORY);
        let _ = std::fs::write(home.user(), ABL_USER);
    }
    // Skills: seed the default skill set only when skills are on.
    if v.skills {
        skills::seed_defaults(&home);
    }
    let brief = if v.brief { Some(ablation_brief_block()) } else { None };
    (home, brief)
}

/// Ablation scenario set — chosen to exercise each system: tool routing (soul/
/// skills shouldn't break it), persona grounding (soul), and knowledge that only
/// MEMORY or the BRIEF carries (deploy command, package manager). `math` is a
/// system-independent control.
fn ablation_scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "route:single",
            user: "Show me the disk usage on my server.",
            targets: 1,
            casual: false,
            conversation: false,
            expect: Expect::ToolOneOf(&["run_command", "run_command_all", "list_vps_targets"]),
        },
        Scenario {
            name: "route:all",
            user: "Check uptime on all of my servers.",
            targets: 2,
            casual: false,
            conversation: false,
            expect: Expect::ToolOneOf(&["run_command_all", "run_command", "list_vps_targets"]),
        },
        Scenario {
            name: "route:in-chat",
            user: "Just show me, in chat, a bash one-liner to count lines in a file. Don't run anything.",
            targets: 1,
            casual: false,
            conversation: false,
            expect: Expect::NoTools,
        },
        Scenario {
            name: "persona",
            user: "In one sentence: who are you and what do you help with?",
            targets: 0,
            casual: false,
            conversation: false,
            // Soul grounds the identity; without it the model gives a generic answer.
            expect: Expect::ContainsAny(&["xconsole", "devops", "server", "infrastructure", "vps"]),
        },
        Scenario {
            name: "know:deploy",
            user: "Without running anything, give me the exact one-line command to deploy this project's app.",
            targets: 1,
            casual: false,
            conversation: false,
            // The deploy command lives in the project brief (and memory).
            expect: Expect::Contains("pm2"),
        },
        Scenario {
            name: "know:pkgmgr",
            user: "Without running anything, what command installs this project's dependencies? Just the command.",
            targets: 1,
            casual: false,
            conversation: false,
            // Memory (and the brief) say pnpm, never npm.
            expect: Expect::Contains("pnpm"),
        },
        Scenario {
            name: "control:math",
            user: "What is 17 * 23? Just the number.",
            targets: 0,
            casual: false,
            conversation: false,
            expect: Expect::Contains("391"),
        },
    ]
}

/// Aggregate numbers for one variant across all ablation scenarios.
struct VariantAgg {
    name: String,
    passes: usize,
    total: usize,
    ptok_avg: u32,
    ttft_avg: u128,
    total_ms_avg: u128,
    gen_tps: f32,
}

async fn bench_ablation(env: &BenchEnv) -> Value {
    let resolved = match env.resolve() {
        Ok(r) => r,
        Err(e) => return json!({ "mode": "ablation", "error": e }),
    };
    let abl_root = env.root.join("ablation");
    let _ = std::fs::create_dir_all(&abl_root);

    let variants = ablation_variants();
    let scns = ablation_scenarios();

    // Warm the model into VRAM so per-variant latencies reflect steady state.
    println!("\n(warming model…)");
    let warm_home = AgentHome::new(abl_root.join("warm"));
    let (warm_sys, _) = env.build_prompt_with(&warm_home, None, &[], true);
    let _ = one_turn(resolved.provider.as_ref(), &env.model, warm_sys, vec![], "hi").await;

    println!(
        "\n=== ABLATION: soul / memory / skills / project-brief ({} scenarios × {} sample(s)) ===",
        scns.len(),
        env.samples
    );

    let mut variant_aggs: Vec<VariantAgg> = Vec::new();
    let mut per_variant_json: Vec<Value> = Vec::new();

    for v in &variants {
        let (home, brief) = seed_variant_home(&abl_root, v);
        println!(
            "\n--- variant {:<8} (soul={} memory={} skills={} brief={}) ---",
            v.name, v.soul as u8, v.memory as u8, v.skills as u8, v.brief as u8
        );
        println!(
            "{:<14} {:>6} {:>8} {:>8} {:>7} {:>6}  {}",
            "scenario", "pass", "ttft_ms", "total_ms", "gen_t/s", "ptok", "selected"
        );

        let mut passes = 0usize;
        let mut ptok_sum = 0u64;
        let mut ttft_sum = 0u128;
        let mut total_sum = 0u128;
        let mut gen_tps_last = 0.0f32;
        let mut turns = 0u128;
        let mut scn_json: Vec<Value> = Vec::new();

        for s in &scns {
            let targets: Vec<String> = (0..s.targets).map(|i| format!("vps-{i}")).collect();
            let mut k = 0usize;
            let mut s_ttft = 0u128;
            let mut s_total = 0u128;
            let mut s_ptok = 0u32;
            let mut s_gen = 0.0f32;
            let mut last_selected = String::new();
            for _ in 0..env.samples {
                let (system, tool_defs) =
                    env.build_prompt_with(&home, brief.clone(), &targets, s.casual);
                let r = one_turn(resolved.provider.as_ref(), &env.model, system, tool_defs, s.user)
                    .await;
                if score(&s.expect, &r) {
                    k += 1;
                }
                s_ttft += r.ttft_ms;
                s_total += r.total_ms;
                s_ptok = r.prompt_tokens;
                s_gen = r.gen_tps;
                last_selected = if r.tool_calls.is_empty() {
                    r.error
                        .as_ref()
                        .map(|e| format!("ERROR: {}", e.chars().take(30).collect::<String>()))
                        .unwrap_or_else(|| "(text)".to_string())
                } else {
                    r.tool_calls.join(",")
                };
            }
            let n = env.samples as u128;
            let ok = k * 2 > env.samples;
            if ok {
                passes += 1;
            }
            ptok_sum += s_ptok as u64;
            ttft_sum += s_ttft;
            total_sum += s_total;
            gen_tps_last = s_gen;
            turns += n;
            println!(
                "{:<14} {:>6} {:>8} {:>8} {:>7.1} {:>6}  {}",
                s.name,
                format!("{k}/{}", env.samples),
                s_ttft / n,
                s_total / n,
                s_gen,
                s_ptok,
                last_selected
            );
            scn_json.push(json!({
                "scenario": s.name,
                "pass": ok,
                "passed_samples": k,
                "prompt_tokens": s_ptok,
                "ttft_ms_avg": s_ttft / n,
                "total_ms_avg": s_total / n,
                "last_selected": last_selected,
            }));
        }

        let nscn = scns.len().max(1) as u64;
        let agg = VariantAgg {
            name: v.name.to_string(),
            passes,
            total: scns.len(),
            ptok_avg: (ptok_sum / nscn) as u32,
            ttft_avg: if turns > 0 { ttft_sum / turns } else { 0 },
            total_ms_avg: if turns > 0 { total_sum / turns } else { 0 },
            gen_tps: gen_tps_last,
        };
        println!(
            "variant {:<8} PASS {}/{}  ptok~{}  ttft~{}ms  total~{}ms",
            v.name, agg.passes, agg.total, agg.ptok_avg, agg.ttft_avg, agg.total_ms_avg
        );
        per_variant_json.push(json!({
            "variant": v.name,
            "soul": v.soul, "memory": v.memory, "skills": v.skills, "brief": v.brief,
            "pass": agg.passes, "total": agg.total,
            "prompt_tokens_avg": agg.ptok_avg,
            "ttft_ms_avg": agg.ttft_avg,
            "total_ms_avg": agg.total_ms_avg,
            "gen_tps": agg.gen_tps,
            "scenarios": scn_json,
        }));
        variant_aggs.push(agg);
    }

    // Per-system contribution = full − ablated. +Δpass means the system HELPS
    // quality; Δptok is the prompt-token cost the system adds to every turn.
    let full = variant_aggs.iter().find(|a| a.name == "full");
    let mut contrib_json: Vec<Value> = Vec::new();
    if let Some(full) = full {
        println!("\n=== PER-SYSTEM CONTRIBUTION (full − without) ===");
        println!(
            "{:<9} {:>7} {:>9} {:>9} {:>10}",
            "system", "Δpass", "Δptok", "Δttft_ms", "Δtotal_ms"
        );
        for (sys, vname) in [
            ("soul", "-soul"),
            ("memory", "-memory"),
            ("skills", "-skills"),
            ("brief", "-brief"),
        ] {
            if let Some(ab) = variant_aggs.iter().find(|a| a.name == vname) {
                let dpass = full.passes as i64 - ab.passes as i64;
                let dptok = full.ptok_avg as i64 - ab.ptok_avg as i64;
                let dttft = full.ttft_avg as i64 - ab.ttft_avg as i64;
                let dtotal = full.total_ms_avg as i64 - ab.total_ms_avg as i64;
                println!(
                    "{:<9} {:>+7} {:>+9} {:>+9} {:>+10}",
                    sys, dpass, dptok, dttft, dtotal
                );
                contrib_json.push(json!({
                    "system": sys,
                    "delta_pass": dpass,
                    "delta_prompt_tokens": dptok,
                    "delta_ttft_ms": dttft,
                    "delta_total_ms": dtotal,
                }));
            }
        }
        if let Some(bare) = variant_aggs.iter().find(|a| a.name == "bare") {
            println!(
                "\nfull: {}/{} pass @ {} ptok   bare (no systems): {}/{} pass @ {} ptok   \
                 → all four systems together add {} prompt tokens and {:+} passes",
                full.passes, full.total, full.ptok_avg,
                bare.passes, bare.total, bare.ptok_avg,
                full.ptok_avg as i64 - bare.ptok_avg as i64,
                full.passes as i64 - bare.passes as i64,
            );
        }
    }

    json!({
        "mode": "ablation",
        "model": env.model,
        "num_ctx": env.num_ctx,
        "samples": env.samples,
        "variants": per_variant_json,
        "per_system_contribution": contrib_json,
    })
}

// ---- Raw LLM latency -----------------------------------------------------

async fn bench_llm(env: &BenchEnv) -> Value {
    let resolved = match env.resolve() {
        Ok(r) => r,
        Err(e) => return json!({ "mode": "llm", "error": e }),
    };
    println!("\n=== RAW LLM LATENCY ===");
    println!(
        "{:<22} {:>8} {:>8} {:>7} {:>6} {:>5}",
        "case", "ttft_ms", "total_ms", "gen_t/s", "ptok", "gtok"
    );

    // Warm-up (load model into VRAM; not timed).
    let (warm_sys, _) = env.build_prompt(&[], true, false);
    let _ = one_turn(resolved.provider.as_ref(), &env.model, warm_sys, vec![], "hi").await;

    let cases: Vec<(&str, Vec<String>, bool, &str)> = vec![
        ("short-no-tools", vec![], true, "In one sentence, what is a reverse proxy?"),
        ("short-with-tools", vec!["vps-0".into()], false, "In one sentence, what is a reverse proxy?"),
        ("full-agent-turn", vec!["vps-0".into(), "vps-1".into()], false, "Summarize what nginx does, briefly."),
    ];

    let mut rows = Vec::new();
    for (name, targets, casual, prompt) in cases {
        let (system, tool_defs) = env.build_prompt(&targets, casual, false);
        let with_tools = !tool_defs.is_empty();
        let r = one_turn(resolved.provider.as_ref(), &env.model, system, tool_defs, prompt).await;
        println!(
            "{:<22} {:>8} {:>8} {:>7.1} {:>6} {:>5}",
            name, r.ttft_ms, r.total_ms, r.gen_tps, r.prompt_tokens, r.completion_tokens
        );
        rows.push(json!({
            "case": name,
            "with_tools": with_tools,
            "ttft_ms": r.ttft_ms,
            "total_ms": r.total_ms,
            "gen_tps": r.gen_tps,
            "prompt_tokens": r.prompt_tokens,
            "completion_tokens": r.completion_tokens,
            "error": r.error,
        }));
    }
    json!({ "mode": "llm", "model": env.model, "num_ctx": env.num_ctx, "cases": rows })
}

fn merge_reports(into: &mut Value, other: Value) {
    if let (Some(obj), Value::Object(o2)) = (into.as_object_mut(), other) {
        obj.insert("agent".to_string(), Value::Object(o2));
    }
}

// ---- Hooks overhead benchmark (no model needed) --------------------------

/// Measure what a Claude Code–style hook costs the agent loop: the pure config/select
/// path (nanoseconds) and a real no-op hook subprocess (the per-tool-call latency a
/// configured PreToolUse hook adds). No Ollama, fully headless.
async fn bench_hooks(out: Option<String>) -> i32 {
    use crate::ai::hooks::{self, HookEvent, HookEventInput, HooksConfig};

    println!("\n=== HOOKS OVERHEAD ===");

    // 1) Pure path: config.select() — what runs on EVERY tool call to decide whether a
    //    hook even fires. Should be negligible.
    let cfg = HooksConfig::parse(
        r#"{"PreToolUse":[{"matcher":"run_command|write_file","hooks":[{"command":"exit 0"}]}]}"#,
    )
    .expect("valid config");
    let iters = 200_000u32;
    let t0 = Instant::now();
    let mut acc = 0usize;
    for _ in 0..iters {
        acc += cfg.select(HookEvent::PreToolUse, Some("run_command")).len();
    }
    std::hint::black_box(acc);
    let pure_ns = t0.elapsed().as_nanos() / iters as u128;
    println!("pure select() per call : {pure_ns} ns   ({iters} iters)");

    // 2) Live path: spawn a no-op hook (exit 0) through the real runner, JSON piped to
    //    stdin. This is the latency a configured PreToolUse hook adds to one tool call.
    let targets: Vec<String> = vec![];
    let args = json!({ "command": "ls -la" });
    let input = HookEventInput {
        event: HookEvent::PreToolUse,
        session_id: "bench",
        cwd: ".",
        workspace_id: None,
        vps_targets: &targets,
        tool_name: Some("run_command"),
        tool_input: Some(&args),
        tool_response: None,
        prompt: None,
    };
    // Warm the shell once (the very first spawn pays a one-off OS cost).
    let _ = hooks::run_event(&cfg, &input).await;
    let runs = 30u32;
    let t1 = Instant::now();
    for _ in 0..runs {
        let _ = hooks::run_event(&cfg, &input).await;
    }
    let live_ms = t1.elapsed().as_millis() as f64 / runs as f64;
    println!("live no-op hook spawn  : {live_ms:.2} ms   ({runs} runs)");

    // 3) Blocking hook (exit 2): confirm the block path works and costs the same order.
    let block_cfg =
        HooksConfig::parse(r#"{"PreToolUse":[{"matcher":"*","hooks":[{"command":"exit 2"}]}]}"#)
            .unwrap();
    let blocked = hooks::run_event(&block_cfg, &input).await.blocks();
    println!("blocking hook (exit 2) : blocks = {blocked}");

    println!(
        "\nA tool call with a PreToolUse hook adds ~{live_ms:.1} ms (one process spawn). \
         With no hooks configured the loop skips the hook path entirely (0 ms)."
    );

    let report = json!({
        "mode": "hooks",
        "pure_select_ns": pure_ns,
        "live_hook_ms": live_ms,
        "live_runs": runs,
        "block_works": blocked,
    });
    if let Some(path) = out {
        match std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap_or_default()) {
            Ok(()) => println!("\nWrote results → {path}"),
            Err(e) => eprintln!("bench: could not write {path}: {e}"),
        }
    }
    if blocked {
        0
    } else {
        1
    }
}

// ---- Self-test (pure logic; runs without Ollama) -------------------------

/// Live hooks self-test: spawns real hook subprocesses through the runner (so it can't
/// live in the sync `selftest()`). Returns 0 on success, 1 on any failure.
async fn selftest_hooks_live() -> i32 {
    use crate::ai::hooks::{self, HookEvent, HookEventInput};

    println!("\n=== SELFTEST: hooks live runner (spawns real subprocesses) ===");
    let mut ok = true;
    let mut check = |name: &str, cond: bool| {
        if cond {
            println!("  PASS {name}");
        } else {
            println!("  FAIL {name}");
            ok = false;
        }
    };

    let targets: Vec<String> = vec![];
    let args = json!({ "command": "ls" });
    let mk = |event| HookEventInput {
        event,
        session_id: "selftest",
        cwd: ".",
        workspace_id: None,
        vps_targets: &targets,
        tool_name: Some("run_command"),
        tool_input: Some(&args),
        tool_response: None,
        prompt: None,
    };

    let block =
        hooks::HooksConfig::parse(r#"{"PreToolUse":[{"matcher":"*","hooks":[{"command":"exit 2"}]}]}"#)
            .unwrap();
    check(
        "PreToolUse exit-2 hook blocks the tool",
        hooks::run_event(&block, &mk(HookEvent::PreToolUse)).await.blocks(),
    );

    let allow =
        hooks::HooksConfig::parse(r#"{"PreToolUse":[{"matcher":"*","hooks":[{"command":"exit 0"}]}]}"#)
            .unwrap();
    check(
        "PreToolUse exit-0 hook allows the tool",
        !hooks::run_event(&allow, &mk(HookEvent::PreToolUse)).await.blocks(),
    );

    let empty = hooks::HooksConfig::default();
    check(
        "no hooks configured → no-op decision",
        !hooks::run_event(&empty, &mk(HookEvent::PreToolUse)).await.blocks(),
    );

    if ok {
        0
    } else {
        1
    }
}

fn selftest() -> i32 {
    use crate::ai::provider::ToolCall;
    use crate::ai::reflection;

    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut check = |name: &str, cond: bool| {
        if cond {
            pass += 1;
            println!("  PASS {name}");
        } else {
            fail += 1;
            println!("  FAIL {name}");
        }
    };

    let call = |id: &str, name: &str, args: Value| ChatMessage {
        role: "assistant".into(),
        content: String::new(),
        tool_calls: vec![ToolCall { id: id.into(), name: name.into(), arguments: args }],
        tool_call_id: None,
    };

    println!("\n=== SELFTEST: reflection (self-improvement / ETAPA 29) ===");
    let failed = vec![
        ChatMessage::user("run foo"),
        call("t1", "run_command", json!({ "command": "foo" })),
        ChatMessage::tool_result("t1", "error: bash: foo: command not found"),
    ];
    let o = reflection::analyze_turn(&failed, 1, 12);
    check("detects one failed tool call", o.failures.len() == 1);
    check(
        "maps result back to tool name",
        o.failures.first().map(|f| f.tool == "run_command").unwrap_or(false),
    );
    let lessons = reflection::distill_lessons(&o);
    check(
        "distills a lesson with a corrective hint",
        lessons
            .iter()
            .any(|l| l.contains("run_command") && l.to_lowercase().contains("alternative")),
    );

    let clean = vec![ChatMessage::user("hi"), ChatMessage::assistant("hello!")];
    check("clean turn reports no trouble", !reflection::analyze_turn(&clean, 1, 12).had_trouble());

    let thrash = vec![
        call("a", "read_file", json!({ "path": "/x" })),
        ChatMessage::tool_result("a", "ok"),
        call("b", "read_file", json!({ "path": "/x" })),
        ChatMessage::tool_result("b", "ok"),
    ];
    check(
        "detects identical-retry thrashing",
        reflection::analyze_turn(&thrash, 2, 12).repeated_tools == vec!["read_file".to_string()],
    );
    check(
        "detects hitting the iteration cap",
        reflection::analyze_turn(&[ChatMessage::user("x")], 12, 12).hit_max_iters,
    );

    let dir = std::env::temp_dir().join(format!("xc-selftest-{}", std::process::id()));
    let home = AgentHome::new(dir.clone());
    let first = reflection::reflect_and_save(&home, &failed, 1, 12);
    let second = reflection::reflect_and_save(&home, &failed, 1, 12);
    check("saves a new lesson to memory", first.len() == 1);
    check("de-duplicates the same mistake", second.is_empty());
    let mem = crate::ai::memory::load_memory(&home);
    check("lesson is present in MEMORY.md", mem.contains("run_command"));
    let _ = std::fs::remove_dir_all(&dir);

    println!("\n=== SELFTEST: voice prompt is much lighter than the normal prompt ===");
    match BenchEnv::setup("dummy-model", "http://localhost:11434", DEFAULT_CTX, 1) {
        Ok(env) => {
            let (normal, _) = env.build_prompt(&[], false, false);
            let (voice, _) = env.build_prompt(&[], false, true);
            println!("  normal prompt: {} chars   voice prompt: {} chars", normal.len(), voice.len());
            check("voice prompt is <1/2 the size of the normal prompt", voice.len() * 2 < normal.len());
            check("voice prompt drops tool guidance", !voice.contains("run_command_all"));
            env.cleanup();
        }
        Err(e) => check(&format!("env setup ({e})"), false),
    }

    println!("\n=== SELFTEST: at-rest encryption (crypto) ===");
    {
        use crate::crypto;
        let key = crypto::new_data_key();
        let ct = crypto::encrypt(&key, b"chats + workspaces").unwrap_or_default();
        check(
            "AES-256-GCM roundtrip",
            crypto::decrypt(&key, &ct).ok().as_deref() == Some(b"chats + workspaces".as_ref()),
        );
        check(
            "wrong key is rejected",
            crypto::decrypt(&crypto::new_data_key(), &ct).is_err(),
        );
        let salt = crypto::new_salt();
        let dk = crypto::new_data_key();
        let it = crypto::DEFAULT_ITERS;
        let wrapped = crypto::wrap_data_key("correct horse", &salt, it, &dk).unwrap_or_default();
        check(
            "password unwraps the data key",
            crypto::unwrap_data_key("correct horse", &salt, it, &wrapped).ok() == Some(dk),
        );
        check(
            "wrong password is rejected",
            crypto::unwrap_data_key("nope", &salt, it, &wrapped).is_err(),
        );
        check(
            "key derivation is deterministic",
            crypto::derive_key("pw", &salt) == crypto::derive_key("pw", &salt),
        );

        // Lock manifest: build (wrap data key under password) → write → read → unlock.
        match crate::lock::build_manifest("hunter2", &dk, 1) {
            Ok(m) => {
                check(
                    "lock manifest unlocks with right password",
                    crate::lock::unlock(&m, "hunter2").ok() == Some(dk),
                );
                check(
                    "lock manifest rejects wrong password",
                    crate::lock::unlock(&m, "nope").is_err(),
                );
                let ld = std::env::temp_dir().join(format!("xc-lock-{}", std::process::id()));
                let _ = std::fs::create_dir_all(&ld);
                let wrote = crate::lock::write(&ld, &m).is_ok();
                check(
                    "manifest write+read roundtrip",
                    wrote
                        && crate::lock::read(&ld).map(|r| r.wrapped_key) == Some(m.wrapped_key.clone()),
                );
                check("is_lock_enabled true after write", crate::lock::is_lock_enabled(&ld));
                let _ = std::fs::remove_dir_all(&ld);
            }
            Err(e) => check(&format!("build_manifest ({e})"), false),
        }
    }

    println!("\n=== SELFTEST: encrypted DB storage roundtrip ===");
    {
        use crate::storage::Db;
        let dir = std::env::temp_dir().join(format!("xc-encdb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let enc = dir.join("xconsole.db.enc");
        let work = dir.join("xconsole.db");
        let key = crate::crypto::new_data_key();

        match Db::open_encrypted(&enc, &work, &dir, &key) {
            Ok(db) => {
                check("encrypted: opens + is_encrypted", db.is_encrypted());
                db.set_setting("enc_probe", "top-secret-XYZ").ok();
                let _ = db.persist_now_blocking();
                check("encrypted: .enc blob created", enc.exists());
                let blob = std::fs::read(&enc).unwrap_or_default();
                check(
                    "encrypted: blob does NOT contain the plaintext value",
                    !blob.windows(14).any(|w| w == b"top-secret-XYZ"),
                );
                db.finalize_on_exit();
                drop(db); // close the connection so the stale plaintext is deletable

                // Simulate next launch: reopen from the blob (stale plaintext cleaned, fresh decrypt).
                match Db::open_encrypted(&enc, &work, &dir, &key) {
                    Ok(db2) => {
                        check(
                            "encrypted: value survives reopen from blob",
                            db2.get_setting("enc_probe").ok().flatten().as_deref()
                                == Some("top-secret-XYZ"),
                        );
                        db2.finalize_on_exit();
                        drop(db2);
                    }
                    Err(e) => check(&format!("encrypted reopen ({e})"), false),
                }

                // The blob itself must be undecryptable with the wrong key (tested directly:
                // an in-process open would recover from the still-open plaintext working file,
                // which a real process-exit would have removed).
                let wrong = crate::crypto::new_data_key();
                check(
                    "encrypted: blob cannot be decrypted with the wrong key",
                    crate::storage::encrypt::decrypt_to_work(&enc, &dir.join("probe.db"), &wrong)
                        .is_err(),
                );
            }
            Err(e) => check(&format!("open_encrypted ({e})"), false),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    println!("\n=== SELFTEST: SSH key generation (Ed25519, post russh-0.61 upgrade) ===");
    {
        use russh::keys::{decode_secret_key, Algorithm};
        match crate::ssh::keygen::generate_ed25519() {
            Ok(k) => {
                check(
                    "keygen: public key is ssh-ed25519",
                    k.public_openssh.starts_with("ssh-ed25519 "),
                );
                check("keygen: fingerprint is SHA256", k.fingerprint.starts_with("SHA256:"));
                check(
                    "keygen: private PEM decodes back to an Ed25519 key",
                    decode_secret_key(&k.private_pem, None)
                        .map(|d| d.algorithm() == Algorithm::Ed25519)
                        .unwrap_or(false),
                );
                // Seeded from the OS CSPRNG, so two fresh keys must differ.
                let other = crate::ssh::keygen::generate_ed25519()
                    .map(|x| x.public_openssh)
                    .unwrap_or_default();
                check("keygen: two fresh keys differ (randomized)", k.public_openssh != other);
            }
            Err(e) => check(&format!("keygen ({e})"), false),
        }
    }

    println!("\n=== SELFTEST: input guards (SSRF URL filter + multi-target parsing) ===");
    {
        use crate::ai::vps_snapshot::user_asks_multiple_targets;
        use crate::ai::web_tools::validate_public_url;
        // The SSRF guard must classify IPv6 literals — including the IPv4-mapped form
        // that points at the cloud-metadata endpoint — not just bare IPv4.
        check("ssrf: blocks IPv6 link-local", validate_public_url("http://[fe80::1]/").is_err());
        check(
            "ssrf: blocks IPv4-mapped IPv6 metadata",
            validate_public_url("http://[::ffff:169.254.169.254]/latest/meta-data").is_err(),
        );
        check(
            "ssrf: still allows a public https URL",
            validate_public_url("https://wttr.in/Berlin?format=3").is_ok(),
        );
        check(
            "targets: 'when did both reboot' is multi-target",
            user_asks_multiple_targets("when did both reboot"),
        );
        check("targets: bare 'both' is not multi-target", !user_asks_multiple_targets("both"));
    }

    println!("\n=== SELFTEST: hooks config + decision parsing (pure) ===");
    {
        use crate::ai::hooks::{self, HookEvent};
        let cfg = hooks::HooksConfig::parse(
            r#"{"hooks":{"PreToolUse":[{"matcher":"run_command","hooks":[{"command":"exit 2"}]}],"UserPromptSubmit":[{"hooks":[{"command":"echo hi"}]}]}}"#,
        );
        check("parses the Claude Code hooks.json shape", cfg.is_ok());
        if let Ok(cfg) = &cfg {
            check("counts PreToolUse hooks", cfg.count(HookEvent::PreToolUse) == 1);
            check(
                "matcher selects the right tool only",
                cfg.select(HookEvent::PreToolUse, Some("run_command")).len() == 1
                    && cfg.select(HookEvent::PreToolUse, Some("write_file")).is_empty(),
            );
            check(
                "non-tool event ignores the matcher",
                cfg.select(HookEvent::UserPromptSubmit, None).len() == 1,
            );
        }
        check("rejects malformed config", hooks::HooksConfig::parse("not json").is_err());
        check(
            "wildcard matcher matches any tool",
            hooks::matcher_matches(Some("*"), Some("anything")),
        );
        let blocked = hooks::parse_output(HookEvent::PreToolUse, 2, "", "denied");
        check(
            "exit 2 blocks with the stderr reason",
            blocked.blocks() && blocked.reason.as_deref() == Some("denied"),
        );
        let json_block = hooks::parse_output(
            HookEvent::PreToolUse,
            0,
            r#"{"decision":"block","reason":"nope"}"#,
            "",
        );
        check("decision:block is honored", json_block.blocks());
        let ctx = hooks::parse_output(HookEvent::UserPromptSubmit, 0, "extra context", "");
        check(
            "UserPromptSubmit stdout becomes context",
            ctx.additional_context.as_deref() == Some("extra context"),
        );
        let allow = hooks::parse_output(
            HookEvent::PreToolUse,
            0,
            r#"{"hookSpecificOutput":{"permissionDecision":"allow"}}"#,
            "",
        );
        check("permission allow does not block", !allow.blocks());
    }

    println!("\nSELFTEST: {pass} passed, {fail} failed");
    if fail > 0 {
        1
    } else {
        0
    }
}

// ---- Preflight -----------------------------------------------------------

async fn preflight(base: &str, model: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Ollama not reachable at {base}: {e}"))?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let present = v
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter().any(|m| {
                m.get("name").and_then(|n| n.as_str()) == Some(model)
                    || m.get("model").and_then(|n| n.as_str()) == Some(model)
            })
        })
        .unwrap_or(false);
    if !present {
        return Err(format!("model '{model}' is not pulled in Ollama"));
    }
    Ok(())
}
