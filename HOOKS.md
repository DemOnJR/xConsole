# Hooks

xConsole's agent supports **lifecycle hooks** — the same model
[Claude Code](https://docs.claude.com/en/docs/claude-code/hooks) uses. A hook is one
of *your* shell commands that the agent runs at a defined point in a turn. A hook can:

- **block a tool** before it runs (a guardrail),
- **inject context** the model sees this turn,
- **feed a tool's result back** to the model, or
- fire a **side-effect** when the turn ends (notification, formatter, audit log).

Hooks are **opt-in**: with none configured the agent loop skips the hook path entirely
(0 ms overhead). Configure them in **Settings → Hooks**, or edit the file directly.

## Configuration

Hooks live in `hooks.json` in the agent home
(`%APPDATA%\com.xconsole.app\agent\hooks.json` on Windows). The format is Claude Code's
`settings.json` `hooks` block — either wrapped in `"hooks"` or bare:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "run_command|run_command_all",
        "hooks": [
          { "type": "command", "command": "my-guard.sh", "timeout": 30 }
        ]
      }
    ]
  }
}
```

The config is **snapshotted at startup** (exactly like Claude Code), so a mid-session
edit — including one the agent itself might write — only takes effect after **Save & apply**
or **Reload** in Settings → Hooks (or a restart). Toggle the whole system off without
deleting your config via the **Enabled** switch (the `agent.hooks_enabled` setting).

## Events

| Event | When | Can it block? |
|---|---|---|
| `UserPromptSubmit` | before a turn runs | yes — rejects the turn |
| `PreToolUse` | before a tool runs | yes — the tool is not run |
| `PostToolUse` | after a tool runs | feeds a note back to the model |
| `Stop` | when the turn finishes | no (fire-and-forget side-effects) |

`PreToolUse`/`PostToolUse` are **tool-scoped** — their `matcher` selects on the tool
name. `UserPromptSubmit`/`Stop` ignore the matcher.

> xConsole runs the agent's tools itself only for its own providers (Ollama / OpenAI /
> Anthropic). Autonomous CLI providers (Cursor/Codex/OpenCode) do their own tool use, so
> `PreToolUse`/`PostToolUse` don't fire for them; `UserPromptSubmit`/`Stop` still do.

### Matcher

A `matcher` selects which tool a tool-event hook applies to:

- omitted, `""`, or `"*"` → **every** tool
- an exact tool name → that tool (`"run_command"`, `"write_file"`, …)
- `a|b|c` → any of several (`"run_command|run_command_all|local_run_command"`)

(Full regex isn't supported — alternation + wildcard covers the practical cases without
a new dependency.) Common tool names: `run_command`, `run_command_all`, `write_file`,
`read_file`, `local_run_command`, `local_write_file`, `upload_file`, `download_file`,
`web_search`, `web_fetch`, `terminal_send`, `canvas_open_terminal`, `memory_save`.

## Input (stdin)

Each command receives the event as a JSON object on **stdin**:

```json
{
  "session_id": "…",
  "cwd": "…",
  "hook_event_name": "PreToolUse",
  "tool_name": "run_command",
  "tool_input": { "command": "rm -rf /", "vps_id": "…" },
  "tool_response": "…",        // PostToolUse only
  "prompt": "…",               // UserPromptSubmit only
  "workspace_id": "…",         // when a workspace is active
  "vps_targets": ["…"]         // selected VPS ids
}
```

## Output (control protocol)

A hook controls the agent through its **exit code** and/or a **JSON object on stdout**:

| Exit code | Meaning |
|---|---|
| `0` | success. For `UserPromptSubmit`, plain stdout is added to the model's context. |
| `2` | **blocking** error — the tool/prompt is blocked; stderr is the reason shown to the model. |
| other | non-blocking error (logged; the agent proceeds). |

For finer control, print a JSON object on stdout (combinable with the exit code):

```jsonc
{ "decision": "block", "reason": "explained to the model" }
{ "continue": false, "stopReason": "halt the whole turn" }
{ "systemMessage": "shown to the user, not the model" }
{ "hookSpecificOutput": { "additionalContext": "injected for the model" } }
{ "hookSpecificOutput": { "permissionDecision": "deny", "permissionDecisionReason": "…" } }
```

> `permissionDecision: "deny"` blocks the tool. xConsole's **command-approval safety mode
> is independent** of hooks: a hook's `"allow"` does **not** bypass the approval gate (a
> deliberate, safer divergence from Claude Code — a hook can add a guardrail but can't
> silently remove the one the user set).

## Examples

**Block destructive commands on production targets** (`PreToolUse`, exit 2 = block):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "run_command|run_command_all",
        "hooks": [{ "type": "command", "command": "guard-rm.sh" }]
      }
    ]
  }
}
```

```bash
#!/usr/bin/env bash
# guard-rm.sh — read the event, block obviously destructive commands.
cmd="$(jq -r '.tool_input.command // ""')"
case "$cmd" in
  *"rm -rf /"*|*"mkfs"*|*":(){ :|:&};:"*)
    echo "refusing destructive command: $cmd" >&2
    exit 2 ;;
esac
exit 0
```

**Inject a standing reminder every turn** (`UserPromptSubmit`, stdout → context):

```json
{ "hooks": { "UserPromptSubmit": [ { "hooks": [
  { "type": "command", "command": "echo Production servers are read-only unless I say otherwise." }
] } ] } }
```

**Notify when a turn finishes** (`Stop`, side-effect):

```json
{ "hooks": { "Stop": [ { "hooks": [
  { "type": "command", "command": "notify-send 'xConsole' 'Agent finished a turn'" }
] } ] } }
```

## Security

Hooks run shell commands **you** configure, with your account's permissions — the same
trust model as Claude Code. Only add commands you trust. Because the config is
snapshotted at startup, a prompt-injected agent can't add a hook that takes effect in the
same session. The command-approval safety mode still applies to every tool regardless of
hooks.

## Implementation / verification

- Engine: [`src-tauri/src/ai/hooks.rs`](src-tauri/src/ai/hooks.rs) — config parsing,
  matcher matching, and output interpretation are pure (unit-tested); only the runner
  spawns a process. Wired into the agent loop in
  [`ai/agent.rs`](src-tauri/src/ai/agent.rs) (UserPromptSubmit / Stop) and
  [`ai/tools.rs`](src-tauri/src/ai/tools.rs) `dispatch` (Pre/PostToolUse).
- Tests: `xconsole-bench selftest` runs the pure-logic checks **plus live hook
  subprocesses** (exit-2 blocks, exit-0 allows). The `#[cfg(test)]` units in `hooks.rs`
  cover the same logic.
- Benchmark: `xconsole-bench hooks` measures the per-tool-call overhead — see
  [`bench/README.md`](bench/README.md). Baseline: ~135 ns to decide whether a hook fires;
  ~38 ms for one no-op hook subprocess on Windows; **0 ms when no hooks are configured**.
