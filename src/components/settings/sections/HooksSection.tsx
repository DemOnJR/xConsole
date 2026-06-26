import { useEffect, useState } from "react";
import { api, type HooksStatus } from "../../../lib/tauri";
import { useSettingsStore } from "../../../stores/settingsStore";
import { Button, Card, Field, SectionHeader, TextArea, Toggle } from "../ui";

const EXAMPLE = `{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "echo Reminder: production servers are read-only." }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "run_command|run_command_all",
        "hooks": [
          { "type": "command", "command": "exit 0" }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "write_file|local_write_file",
        "hooks": [
          { "type": "command", "command": "exit 0" }
        ]
      }
    ]
  }
}`;

/** Claude Code–style lifecycle hooks: edit hooks.json, toggle the system, see status. */
export function HooksSection() {
  const enabledSetting = useSettingsStore((s) => s.settings["agent.hooks_enabled"]);
  const setSetting = useSettingsStore((s) => s.set);
  const enabled = enabledSetting !== "false";

  const [draft, setDraft] = useState("");
  const [saved, setSaved] = useState("");
  const [status, setStatus] = useState<HooksStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const refreshStatus = () => api.hooksStatus().then(setStatus);

  const load = async () => {
    const [cfg] = await Promise.all([api.getHooksConfig(), refreshStatus()]);
    setDraft(cfg);
    setSaved(cfg);
  };

  useEffect(() => {
    load();
  }, []);

  const dirty = draft !== saved;

  const save = async () => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const count = await api.saveHooksConfig(draft);
      setSaved(draft);
      await refreshStatus();
      setNote(`Saved — ${count} hook${count === 1 ? "" : "s"} active.`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const reload = async () => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      await api.reloadHooks();
      await load();
      setNote("Reloaded from disk.");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <SectionHeader
        title="Hooks"
        description="Run your own shell commands on agent lifecycle events — the same model Claude Code uses. A hook can block a tool before it runs, feed context back to the model, or fire a side-effect (notification, formatter, audit log) when the turn ends."
        action={
          <Toggle
            checked={enabled}
            onChange={(v) => setSetting("agent.hooks_enabled", v ? "true" : "false")}
            label={enabled ? "Enabled" : "Disabled"}
          />
        }
      />

      <Card className="mb-3">
        <div className="mb-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-gray-400">
          <span className="text-gray-300">
            {status ? `${status.total} hook${status.total === 1 ? "" : "s"} loaded` : "…"}
          </span>
          {status && (
            <>
              <span>PreToolUse · {status.pre_tool_use}</span>
              <span>PostToolUse · {status.post_tool_use}</span>
              <span>UserPromptSubmit · {status.user_prompt_submit}</span>
              <span>Stop · {status.stop}</span>
            </>
          )}
        </div>
        {status?.error && (
          <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-2.5 py-1.5 text-[11px] text-amber-200">
            hooks.json on disk has an error (the last valid config is still active): {status.error}
          </div>
        )}
        {!enabled && (
          <div className="text-[11px] text-gray-500">
            Hooks are disabled — no hook commands run, regardless of what's configured below.
          </div>
        )}
      </Card>

      <Card className="mb-3">
        <Field
          label="hooks.json"
          hint="Stored in the agent home. Snapshotted at startup — Save (or Reload) to apply changes mid-session."
        >
          <TextArea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={16}
            spellCheck={false}
            className="font-mono text-[12px]"
            placeholder='{ "hooks": { "PreToolUse": [ … ] } }'
          />
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <Button variant="primary" onClick={save} disabled={!dirty || busy}>
              {busy ? "Saving…" : "Save & apply"}
            </Button>
            <Button onClick={reload} disabled={busy}>
              Reload from disk
            </Button>
            <Button
              onClick={() => setDraft(EXAMPLE)}
              disabled={busy}
              title="Replace the editor with a starter config"
            >
              Insert example
            </Button>
            {dirty ? (
              <span className="text-[11px] text-amber-300">Unsaved changes</span>
            ) : note ? (
              <span className="text-[11px] text-gray-500">{note}</span>
            ) : null}
          </div>
          {error && (
            <div className="mt-2 rounded-md border border-red-500/40 bg-red-500/10 px-2.5 py-1.5 text-[11px] text-red-200">
              {error}
            </div>
          )}
        </Field>
      </Card>

      <Card>
        <div className="mb-1.5 text-xs font-medium text-gray-200">How hooks work</div>
        <ul className="space-y-1 text-[11px] leading-relaxed text-gray-500">
          <li>
            <span className="text-gray-300">Events:</span>{" "}
            <code>PreToolUse</code> (before a tool runs — can block it),{" "}
            <code>PostToolUse</code> (after — can feed the result back),{" "}
            <code>UserPromptSubmit</code> (before the turn — can inject context), and{" "}
            <code>Stop</code> (when the turn finishes).
          </li>
          <li>
            <span className="text-gray-300">matcher:</span> a tool-name pattern for the
            tool events — exact name, <code>*</code> for all, or{" "}
            <code>a|b|c</code> for several (e.g. <code>run_command|write_file</code>).
          </li>
          <li>
            <span className="text-gray-300">Input:</span> each command receives the event
            as JSON on stdin (<code>tool_name</code>, <code>tool_input</code>,{" "}
            <code>prompt</code>, <code>vps_targets</code>, …).
          </li>
          <li>
            <span className="text-gray-300">Control:</span> exit <code>0</code> = allow
            (UserPromptSubmit stdout is added as context); exit <code>2</code> = block,
            with stderr as the reason. Or print JSON:{" "}
            <code>{`{"decision":"block","reason":"…"}`}</code> /{" "}
            <code>{`{"hookSpecificOutput":{"permissionDecision":"deny"}}`}</code>.
          </li>
          <li className="text-amber-300/80">
            Hooks run shell commands you configure, with your account's permissions — only
            add commands you trust.
          </li>
        </ul>
      </Card>
    </div>
  );
}
