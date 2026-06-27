# xConsole benchmarks

Regression benchmarks for the agent + local-model path. Run them, make a change,
run them again, and compare — so every feature is measured, not guessed.

All numbers below were taken on the dev machine (Ryzen 9 5900X, **AMD RX 9060 XT
16 GB via ROCm**, Ollama `qwen3.5:9b`). Your mileage varies with hardware and load.

## 1. `xconsole-bench` — full agent eval (recommended)

A headless binary that drives the **real** code path: the same three-tier system
prompt, the same curated Ollama tool schema, and the same `Provider::chat` the
desktop app uses — with no webview. A running xConsole can't lock it (separate exe).

Build (GNU toolchain, MinGW on PATH — see `src-tauri/AGENTS.md`):

```bash
cargo +stable-x86_64-pc-windows-gnu build --release --bin xconsole-bench --manifest-path src-tauri/Cargo.toml
```

Run:

```bash
# Agent quality + latency eval — 3 samples/scenario smooths Ollama's non-determinism
./src-tauri/target/release/xconsole-bench.exe agent --model qwen3.5:9b --samples 3 --out bench/results/agent.json

# Raw model latency (TTFT / gen tok/s) with and without the tool payload
./src-tauri/target/release/xconsole-bench.exe llm   --model qwen3.5:9b

# Pure-logic self-tests (reflection + voice prompt + hooks) + live hook subprocesses — no Ollama
./src-tauri/target/release/xconsole-bench.exe selftest

# Hooks dispatch overhead — what a PreToolUse hook adds per tool call (no Ollama)
./src-tauri/target/release/xconsole-bench.exe hooks --out bench/results/hooks.json

# Both eval + latency
./src-tauri/target/release/xconsole-bench.exe all   --model qwen3.5:9b --out bench/results/all.json
```

Flags: `--model <tag>` (default `qwen3.5:9b`), `--base <url>` (default
`http://localhost:11434`), `--ctx <n>` (default `65536`, the app default),
`--samples <n>` (default 1; use 3+ for a stable pass-rate), `--out <path>`.

> The app sends **no seed/temperature** to Ollama, so tool-vs-text decisions are
> non-deterministic. A scenario passes when a **strict majority** of its samples pass;
> run `--samples 3` (or more) for a trustworthy number. The `pass` column shows `k/N`.

**What the agent eval checks** — each scenario asserts a behavior a useful agent
must get right; the pass-rate is the quality signal we track:

| scenario | asserts |
|---|---|
| casual-greeting | small talk → no tool calls |
| math-no-tools | answers `17*23` = 391 in chat, no tools |
| voice-terse-explain | one-sentence answer, no tools |
| single-server-command | picks `run_command` for one server |
| all-servers-command | picks `run_command_all` for "all servers" |
| write-remote-file | picks `write_file` for `/root/hello.py` |
| in-chat-code-only | "just show me, don't run" → no tools |

Columns: `ttft_ms` (time to first token), `total_ms` (whole turn), `gen_t/s`
(generation tokens/sec), `ptok` (prompt tokens — how heavy the system prompt is).

**Hooks overhead** (`hooks` mode) — measures the cost of the Claude Code–style hooks
system (see [`HOOKS.md`](../HOOKS.md)):

| metric | meaning | dev-machine baseline |
|---|---|---|
| `pure_select_ns` | per-tool-call cost to decide whether a hook fires | ~135 ns |
| `live_hook_ms` | one no-op hook subprocess (spawn + JSON on stdin) | ~38 ms (Windows `cmd /C`) |

With **no hooks configured the loop skips the hook path entirely (0 ms)** — hooks are
opt-in, so they cost nothing until you add one. The `live_hook_ms` figure is dominated
by process-spawn latency (lower on Unix `sh -c`); a hook that does real work adds its own time.

## 1a. Harder suites — `hard` and `recall`

The core `agent` suite saturates at 100% on `qwen3.5:9b`, so it no longer
discriminates. Two harder, **scored** suites add headroom (so the history can show
learning/regressions):

```bash
# Discriminative agent suite — tool-boundary routing traps + adversarial
# action-vs-explain restraint (a 9B does NOT ace these). Reports an overall pass-rate
# (with a Wilson CI) and a per-tier breakdown (hard / expert).
./src-tauri/target/release/xconsole-bench.exe hard --samples 3

# Reasoning-unlocks-recall experiment — single-hop factual questions answered three
# ways: direct, reason-first, and a dummy "Let me think" buffer.
./src-tauri/target/release/xconsole-bench.exe recall --samples 3

# Closed learning loop — answer unfamiliar-tool tasks COLD (memory), let the
# autoresearch loop build a skill for each, then answer WARM (skill injected).
# Records cold/warm as two history points so the dashboard shows the before/after.
./src-tauri/target/release/xconsole-bench.exe learnloop --samples 3
```

`learnloop` is the experiment that asks "does the agent actually get better by
learning?" — cold vs warm pass-rate, plus a persistence check (a second `learn()`
must dedup to the existing skill, no re-research). It also honestly catches the
failure mode: a low-quality researched skill can *regress* a task, which is why
draft skills stay quarantined until execution-verified.

`recall` tests Google Research's *"Thinking to Recall: how reasoning unlocks parametric
knowledge in LLMs"* on our local model: does a reasoning trace surface facts the model
has in its weights but can't recall when answering directly? It reports `direct`,
`reason`, and `buffer` accuracy and the **reasoning gain** (`reason − direct`). Per the
paper, a large positive gain means reasoning unlocks recall (factual priming); a gain
from the dummy `buffer` condition isolates the pure compute-buffer effect; a *negative*
gain flags the paper's failure mode (a hallucinated intermediate fact derailing the
answer). The `hard`/`recall` scenarios were generated and adversarially fact-checked by
a multi-agent workflow so their expected answers are correct.

## 1b. Benchmark history — scores over time (HTML dashboard + OKF bundle)

Every **scored** run (`agent`, `hard`, `recall`, `ablation`, `learn`, `llm`, `all`) is
appended to `bench/results/history.jsonl` and rendered two ways automatically:

- **`bench/results/history.html`** — a self-contained dashboard (open it in any
  browser; no server, no external assets) charting pass-rate and latency over time,
  with a **Wilson 95% confidence interval** on every pass-rate.
- **`bench/history/`** — the same history as an **[Open Knowledge Format](https://github.com/GoogleCloudPlatform/knowledge-catalog/tree/main/okf)**
  bundle (Google's portable markdown+YAML standard): one typed concept per run
  (`runs/*.md`), a chronological `log.md`, and an `index.md`. Portable, vendor-neutral,
  readable in any editor and on GitHub.

```bash
# Rebuild the dashboard + OKF bundle from the existing history (no model needed):
./src-tauri/target/release/xconsole-bench.exe report

# Skip recording a run (e.g. a throwaway/tuning run):
./src-tauri/target/release/xconsole-bench.exe agent --no-history
```

**Methodology** (applied + cited in the dashboard footer):

- **Confidence intervals, not point estimates.** A pass-rate from a few samples is
  noisy — 3–5 samples is *often insufficient* and the same source can wander ±1 pass.
  Each pass-rate is reported with a Wilson 95% CI; when two runs' intervals overlap,
  the difference isn't real. (Google Research, *"Building better AI benchmarks: how
  many raters are enough?"* — more items beats more samples for an accuracy metric.)
- **`time for 100 output tokens` = TTFT + 100 / (tok/s)** — one comparable latency
  number across runs. (Artificial Analysis methodology.)
- **Revealed behavior vs. self-report.** The learn-loop eval measures what the model
  *does* (does it route to `learn_skill`?) against what it *claims* (the classifier's
  self-assessment) — the gap is the model's overconfidence. (Google Research,
  *"Evaluating alignment of behavioral dispositions in LLMs."*)

## 2. `ollama_latency.ps1` — zero-build latency probe

Quick TTFT / tok/s read without compiling, straight against `/api/chat`:

```powershell
pwsh bench/ollama_latency.ps1 -Model qwen3.5:9b -Out bench/results/llm.json
```

It also shows `think=ON` vs `OFF` (thinking mode ~5×'s latency — keep it off for
voice) and a heavy-system-prompt case (shows how much the big prompt costs TTFT).

## Notes

- Ollama must be running (`ollama serve`) and the model pulled (`ollama pull qwen3.5:9b`).
- Results are **non-deterministic** — the app sends no fixed seed/temperature to
  Ollama, so re-run a few times and compare medians. Latency also depends on what
  else is using the GPU (a resident `llama-server` or a game can force VRAM spillover).
