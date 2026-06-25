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
//!   xconsole-bench agent  [--model qwen3.5:9b] [--base http://localhost:11434] [--ctx 65536] [--out results.json]
//!   xconsole-bench llm    [--model ...] [--ctx ...]
//!   xconsole-bench all
//!
//! These are REGRESSION benchmarks: run them, change a feature, run them again,
//! and compare the JSON to see whether latency / pass-rate improved.

use std::path::PathBuf;
use std::time::Instant;

use serde_json::{json, Value};

use crate::ai::context::{self, PromptContext};
use crate::ai::provider::{ChatMessage, ChatRequest, Provider, StreamEvent, StreamStats, ToolDef};
use crate::ai::registry::{self, ResolvedProvider};
use crate::ai::{skills, tools, AgentHome};
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

    // Pure-logic self-tests (reflection, voice prompt) — no Ollama needed.
    if mode == "selftest" {
        return selftest();
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
        "all" => {
            let mut a = bench_llm(&env).await;
            let b = bench_agent(&env).await;
            merge_reports(&mut a, b);
            a
        }
        other => {
            eprintln!("bench: unknown mode '{other}' (use: agent | llm | all)");
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

// ---- Self-test (pure logic; runs without Ollama) -------------------------

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
