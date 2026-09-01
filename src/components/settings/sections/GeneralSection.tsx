import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "../../../lib/tauri";
import { useSettingsStore } from "../../../stores/settingsStore";
import { useUpdateStore, type UpdateChannel } from "../../../stores/updateStore";
import { Field, SectionHeader, Select, TextInput, Toggle, SettingsGroup, SettingsRow, Button } from "../ui";

export const SK = {
  agentEnabled: "agent.enabled",
  activeProvider: "agent.active_provider",
  safetyMode: "agent.safety_mode",
  externalEditor: "sftp.external_editor",
  toolResultMaxChars: "agent.tool_result_max_chars",
  cacheRetention: "agent.cache_retention",
  visionMode: "agent.vision_mode",
  visionProvider: "agent.vision_provider",
  visionModel: "agent.vision_model",
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
  const [autostart, setAutostart] = useState<{ enabled: boolean; supported: boolean } | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
    void loadChannel();
    api.getAutostart()
      .then(setAutostart)
      .catch((e) => setAutostartError(String(e)));
  }, [loadChannel]);
  const checking = updateStatus === "checking" || updateStatus === "updating";

  return (
    <div className="space-y-6">
      <SectionHeader
        title="General Preferences"
        description="System defaults, AI assistant state, remote editor bindings, and release channels."
      />

      <SettingsGroup title="Application">
        <SettingsRow
          label="Launch at Windows sign-in"
          description={
            autostart && !autostart.supported
              ? "Only available on Windows. Adds xConsole to this user's startup list (no admin)."
              : "Open xConsole when you sign in to Windows. Per-user, no administrator rights."
          }
        >
          <Toggle
            checked={autostart?.enabled ?? false}
            disabled={!autostart || !autostart.supported}
            onChange={(v) => {
              setAutostartError(null);
              void api
                .setAutostart(v)
                .then(setAutostart)
                .catch((e) => setAutostartError(String(e)));
            }}
          />
        </SettingsRow>
        {autostartError && (
          <p className="text-[11px] text-red-400">{autostartError}</p>
        )}
      </SettingsGroup>

      <SettingsGroup title="AI Assistant" className="pt-4 border-t border-[var(--border)]">
        <SettingsRow
          label="Autonomous AI Agent"
          description="Enable or disable the AI pairing engine across the workspace canvas."
        >
          <Toggle
            checked={agentEnabled}
            onChange={(v) => set(SK.agentEnabled, v ? "true" : "false")}
          />
        </SettingsRow>

        <Field
          label="Default AI Provider"
          hint="The default provider new agent sessions use. Configure API keys in the AI Providers tab."
        >
          <Select
            value={activeProvider}
            onChange={(e) => set(SK.activeProvider, e.target.value)}
          >
            <option value="">(None selected - prompt when opening agent)</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} ({p.kind})
              </option>
            ))}
          </Select>
        </Field>
      </SettingsGroup>

      <SettingsGroup
        title="Editor & Tooling"
        className="pt-4 border-t border-[var(--border)]"
      >
        <Field
          label="External Editor for Remote Files"
          hint="Leave empty to use built-in editor. Set to 'code' for VS Code (or custom editor binary path). Remote files open externally and automatically sync changes back over SFTP."
        >
          <TextInput
            value={settings[SK.externalEditor] ?? ""}
            onChange={(e) => void set(SK.externalEditor, e.target.value)}
            placeholder="code"
            spellCheck={false}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup
        title="Software Updates & Channel"
        className="pt-4 border-t border-[var(--border)]"
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <Field
            label="Release Channel"
            hint="Stable tracks verified releases. Dev tracks latest pre-release builds."
          >
            <Select
              value={channel}
              disabled={checking}
              onChange={(e) => void setChannel(e.target.value as UpdateChannel)}
            >
              <option value="main">Stable (main branch)</option>
              <option value="dev">Dev (dev branch)</option>
            </Select>
          </Field>

          <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3 flex flex-col justify-between">
            <div>
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold text-gray-200">v{appVersion || "0.1.0"}</span>
                <span className="rounded bg-cyan-500/20 border border-cyan-500/30 px-1.5 py-0.2 text-[9px] font-mono font-semibold uppercase text-cyan-300">
                  {channel === "dev" ? "dev" : "stable"}
                </span>
                {current && (
                  <span className="text-[10px] font-mono text-gray-500" title="Git commit SHA">
                    {current.slice(0, 7)}
                  </span>
                )}
              </div>
              {note && <p className="mt-1 text-[11px] text-amber-300">{note}</p>}
              {localBranch && localBranch !== channel && (
                <p className="mt-1 text-[10px] text-amber-400 font-mono">
                  Branch mismatch: on {localBranch}, channel {channel}
                </p>
              )}
            </div>

            <div className="mt-3 flex justify-end">
              <Button
                variant="ghost"
                onClick={() => void checkUpdate(true)}
                disabled={checking}
              >
                {updateStatus === "checking" ? "Checking…" : "Check for Updates"}
              </Button>
            </div>
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
}
