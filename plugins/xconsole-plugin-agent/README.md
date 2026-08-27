# Autonomous AI Agent Engine for xConsole 🤖

The autonomous pairing assistant and harness engine for **xConsole**, featuring multi-provider streaming, real-time tool execution, diff inspections, safety gates, and goal loops.

---

## ⚡ Key Features

- **Multi-Provider AI Core**: Seamless streaming with Anthropic (Claude 3.5 Sonnet / Opus), OpenAI (GPT-5, 4o), DeepSeek (V3, R1), Google Gemini, xAI Grok, Ollama, and 20+ LLM providers.
- **Real-Time Tool Execution & Streaming**:
  - `execute_bash`: Run terminal commands with streamed output.
  - `edit_file`: Exact surgical search-and-replace edits with syntax checks.
  - `read_file` & `write_file`: File inspection and creation.
  - `search_code`: Workspace ripgrep searching.
  - `fetch_web_page`: Markdown web fetcher.
  - `ask_user`: Interactive clarifying questions.
  - `present_plan`: Implementation plans with diff reviews.
- **Collapsible Action Cards**: Grouped summaries (`executed 4 commands · read 3 files · wrote 3 files · 10 actions`).
- **Interactive Input Bar**:
  - Multi-line prompt with command auto-complete (`/`).
  - Image paste & drag-and-drop vision support.
  - Runtime mode selector (🤖 Auto · 📋 Plan · ⚡ Code · 🌐 Standard · 🛡️ Minimal).
  - Safety permission profiles (Autonomous, Prompt before write, Read-only).
  - Live context window gauge & speed metrics (`t/s`).
- **Voice & TTS**: Voice dictation and speech output.
- **Goal Loops**: Background goal ticks and task orchestration (`/goal`).

---

## 🚀 Installation

```bash
xconsole plugin install DemOnJR/xconsole-plugin-agent
```
