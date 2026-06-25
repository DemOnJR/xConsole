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

# Pure-logic self-tests (reflection / self-improvement + voice prompt) — no Ollama needed
./src-tauri/target/release/xconsole-bench.exe selftest

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
