import { useEffect } from "react";
import { useSettingsStore } from "../../../stores/settingsStore";
import { useVpsStore } from "../../../stores/vpsStore";
import { defaultVisionModel, isGeminiProvider, parseVisionMode } from "../../../lib/vision";
import { Field, SectionHeader, Select, TextInput, SettingsGroup } from "../ui";
import { SK } from "./GeneralSection";

const SAFETY_OPTIONS: { value: string; label: string; hint: string }[] = [
  {
    value: "approve",
    label: "Approve each action (Default / Recommended)",
    hint: "Every command, file edit, and critical tool execution must be approved before running.",
  },
  {
    value: "allowlist",
    label: "Allowlist safe commands",
    hint: "Auto-runs read-only and inspection commands; prompts for destructive or write operations.",
  },
  {
    value: "full",
    label: "Full autonomy",
    hint: "Executes all commands directly without manual confirmation.",
  },
];

export function AgentSection() {
  const settings = useSettingsStore((s) => s.settings);
  const set = useSettingsStore((s) => s.set);
  const providers = useSettingsStore((s) => s.providers);
  const vpsList = useVpsStore((s) => s.vpsList);
  const loadVps = useVpsStore((s) => s.load);

  useEffect(() => {
    loadVps();
  }, [loadVps]);

  const global = settings[SK.safetyMode] ?? "approve";
  const currentHint = SAFETY_OPTIONS.find((o) => o.value === global)?.hint;

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Agent Engine & Safety Gates"
        description="Control agent autonomy level, vision analysis, prompt token cache TTL, and per-server security policies."
      />

      <SettingsGroup title="Autonomy & Safety Policy">
        <Field label="Global Safety Mode" hint={currentHint}>
          <Select
            value={global}
            onChange={(e) => set(SK.safetyMode, e.target.value)}
          >
            {SAFETY_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
          <Field
            label="Tool Output Limit (Characters)"
            hint="Maximum character window stored per tool result. Default: 4000. Set 0 for unlimited."
          >
            <TextInput
              type="number"
              min={0}
              step={500}
              value={settings[SK.toolResultMaxChars] ?? "4000"}
              onChange={(e) => set(SK.toolResultMaxChars, e.target.value)}
            />
          </Field>

          <Field
            label="Prompt Cache Retention"
            hint="Prompt prefix caching TTL (Anthropic). Default: 5 minutes."
          >
            <Select
              value={settings[SK.cacheRetention] ?? ""}
              onChange={(e) => set(SK.cacheRetention, e.target.value)}
            >
              <option value="">5 minutes (Standard)</option>
              <option value="long">1 hour (Extended)</option>
            </Select>
          </Field>
        </div>
      </SettingsGroup>

      <SettingsGroup
        title="Vision & Multimodal Analysis"
        description="Image recognition and screenshot analysis capabilities."
        className="pt-4 border-t border-[var(--border)]"
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <Field
            label="Vision Mode"
            hint="Ask = confirm before sending images. Enabled = auto send."
          >
            <Select
              value={parseVisionMode(settings[SK.visionMode])}
              onChange={(e) => set(SK.visionMode, e.target.value)}
            >
              <option value="ask">Ask before sending images</option>
              <option value="enabled">Always send images</option>
              <option value="disabled">Disable image analysis</option>
            </Select>
          </Field>

          <Field
            label="Dedicated Vision Provider"
            hint="Used if the primary model is text-only. Gemini Flash is recommended."
          >
            <Select
              value={settings[SK.visionProvider] ?? ""}
              onChange={(e) => {
                const id = e.target.value;
                void set(SK.visionProvider, id);
                const p = providers.find((x) => x.id === id);
                if (p) void set(SK.visionModel, defaultVisionModel(p));
              }}
            >
              <option value="">Auto (Gemini Flash if configured)</option>
              {providers
                .filter((p) => p.enabled)
                .map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                    {isGeminiProvider(p) ? " (Recommended for Vision)" : ""}
                  </option>
                ))}
            </Select>
          </Field>
        </div>
      </SettingsGroup>

      <SettingsGroup
        title="Per-Host Security Overrides"
        description="Override the default safety gate on specific environments (e.g. Full autonomy on local sandboxes, Approve each on production)."
        className="pt-4 border-t border-[var(--border)]"
      >
        {vpsList.length === 0 ? (
          <p className="text-xs text-gray-500 italic">No remote servers configured yet.</p>
        ) : (
          <div className="space-y-2">
            {vpsList.map((v) => {
              const key = `${SK.safetyMode}.${v.id}`;
              const value = settings[key] ?? "";
              return (
                <div
                  key={v.id}
                  className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--surface)] p-2.5"
                >
                  <span className="truncate text-xs font-mono text-gray-200">
                    {v.name} ({v.host})
                  </span>
                  <Select
                    value={value}
                    onChange={(e) => set(key, e.target.value)}
                    className="w-48"
                  >
                    <option value="">Use Global Default</option>
                    {SAFETY_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </Select>
                </div>
              );
            })}
          </div>
        )}
      </SettingsGroup>
    </div>
  );
}
