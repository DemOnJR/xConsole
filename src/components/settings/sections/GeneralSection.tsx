import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useSettingsStore } from "../../../stores/settingsStore";
import { useUpdateStore, type UpdateChannel } from "../../../stores/updateStore";
import { Card, Field, SectionHeader, Select, TextInput, Toggle } from "../ui";

export const SK = {
  agentEnabled: "agent.enabled",
  activeProvider: "agent.active_provider",
  safetyMode: "agent.safety_mode",
  externalEditor: "sftp.external_editor",
} as const;

export function GeneralSection() {
  const settings = useSettingsStore((s) => s.settings);
  const providers = useSettingsStore((s) => s.providers);
  const set = useSettingsStore((s) => s.set);

  const agentEnabled = settings[SK.agentEnabled] !== "false";
  const activeProvider = settings[SK.activeProvider] ?? "";

  const updateStatus = useUpdateStore((s) => s.status);
  const channel = useUpdateStore((s) => s.channel);
  const current = useUpdateStore((s) => s.current);
  const localBranch = useUpdateStore((s) => s.localBranch);
  const note = useUpdateStore((s) => s.note);
  const checkUpdate = useUpdateStore((s) => s.check);
  const setChannel = useUpdateStore((s) => s.setChannel);
  const loadChannel = useUpdateStore((s) => s.loadChannel);

  const [appVersion, setAppVersion] = useState("");
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
    void loadChannel();
  }, [loadChannel]);
  const checking = updateStatus === "checking" || updateStatus === "updating";

  return (
    <div>
      <SectionHeader
        title="General"
        description="App-wide defaults: agent on/off, active provider, remote editor, and updates."
      />

      <Card className="mb-3">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm text-gray-200">AI agent</div>
            <div className="text-xs text-gray-500">
              Enable the assistant across the app.
            </div>
          </div>
          <Toggle
            checked={agentEnabled}
            onChange={(v) => set(SK.agentEnabled, v ? "true" : "false")}
          />
        </div>
      </Card>

      <Card>
        <Field
          label="Active provider"
          hint="The default model/provider new agent sessions use. Configure providers in the Providers tab."
        >
          <Select
            value={activeProvider}
            onChange={(e) => set(SK.activeProvider, e.target.value)}
          >
            <option value="">(none selected)</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} ({p.kind})
              </option>
            ))}
          </Select>
        </Field>
      </Card>

      <Card className="mt-3">
        <Field
          label="External editor for remote files"
          hint="Leave empty to use the built-in editor. Set to `code` for VS Code (or a full path, plus any flags). Choosing “Open in …” on a file in an SFTP panel downloads it, opens it there, and uploads every save back — verifying the byte count first, and refusing to replace a non-empty file with an empty one."
        >
          <TextInput
            value={settings[SK.externalEditor] ?? ""}
            onChange={(e) => void set(SK.externalEditor, e.target.value)}
            placeholder="code"
            spellCheck={false}
          />
        </Field>
      </Card>

      <Card className="mt-3">
        <Field
          label="Release channel"
          hint="Stable tracks the main branch (production). Dev tracks the dev branch for pre-release builds. Switching channels checks for an update; install it to rebuild from that branch. Your data is never touched."
        >
          <Select
            value={channel}
            disabled={checking}
            onChange={(e) => void setChannel(e.target.value as UpdateChannel)}
          >
            <option value="main">Stable (main)</option>
            <option value="dev">Dev (dev)</option>
          </Select>
        </Field>
      </Card>

      <Card className="mt-3">
        <Field
          label="App version & updates"
          hint="xConsole checks GitHub for newer code on your selected channel and prompts you to update. An update re-clones + rebuilds from source — chats, workspaces, memory, settings, and keys are backed up first and never touched."
        >
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2 text-sm text-gray-300">
              <span>v{appVersion || "…"}</span>
              <span className="rounded bg-[var(--accent-muted)] px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--accent)]">
                {channel === "dev" ? "dev" : "stable"}
              </span>
              {current ? (
                <span className="font-mono text-xs text-gray-500" title="Built from commit">
                  {current}
                </span>
              ) : null}
              {localBranch && localBranch !== channel ? (
                <span className="text-xs text-amber-300">
                  install on {localBranch} · channel {channel}
                </span>
              ) : null}
            </div>
            {note ? <p className="text-[11px] text-amber-300/90">{note}</p> : null}
            <div className="flex items-center justify-end">
              <button
                onClick={() => void checkUpdate(true)}
                disabled={checking}
                className="rounded-md border border-[var(--border)] px-3 py-1.5 text-xs text-gray-200 transition hover:bg-[var(--border)] disabled:cursor-not-allowed disabled:opacity-50"
              >
                {updateStatus === "checking" ? "Checking…" : "Check for updates"}
              </button>
            </div>
          </div>
        </Field>
      </Card>
    </div>
  );
}
