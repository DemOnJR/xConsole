import { useEffect } from "react";
import { useSettingsStore } from "../../../stores/settingsStore";
import { useVpsStore } from "../../../stores/vpsStore";
import { Card, Field, SectionHeader, Select } from "../ui";
import { SK } from "./GeneralSection";

const SAFETY_OPTIONS: { value: string; label: string; hint: string }[] = [
  {
    value: "full",
    label: "Full autonomy",
    hint: "Runs any command on any server with no confirmation.",
  },
  {
    value: "allowlist",
    label: "Allowlist",
    hint: "Auto-runs read-only/safe commands; asks approval for the rest.",
  },
  {
    value: "approve",
    label: "Approve each",
    hint: "Every command must be approved before it runs.",
  },
];

export function AgentSection() {
  const settings = useSettingsStore((s) => s.settings);
  const set = useSettingsStore((s) => s.set);
  const vpsList = useVpsStore((s) => s.vpsList);
  const loadVps = useVpsStore((s) => s.load);

  useEffect(() => {
    loadVps();
  }, [loadVps]);

  const global = settings[SK.safetyMode] ?? "approve";
  const currentHint = SAFETY_OPTIONS.find((o) => o.value === global)?.hint;

  return (
    <div>
      <SectionHeader
        title="Agent & Safety"
        description="Control how much autonomy the agent has when it runs commands on your servers. You can override the default per server."
      />

      <Card className="mb-3">
        <Field label="Default safety mode" hint={currentHint}>
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
      </Card>

      <Card className="mb-3">
        <Field
          label="Tool output limit (chars)"
          hint="Applied once when the tool result is stored (append-only, cache-friendly). Default 4000. 0 = unlimited."
        >
          <input
            type="number"
            min={0}
            step={500}
            value={settings[SK.toolResultMaxChars] ?? "4000"}
            onChange={(e) => set(SK.toolResultMaxChars, e.target.value)}
            className="w-40 rounded border border-[var(--border-strong)] bg-[var(--bg)] px-2 py-1 text-sm text-gray-200 outline-none focus:border-[var(--accent)]"
          />
        </Field>
        <Field
          label="Cache retention"
          hint="Anthropic only. Long = 1h TTL at 2× cache-write price. Leave at 5 minutes unless sessions sit idle. DeepSeek / Command Code ignore this — they auto-cache the prefix."
        >
          <Select
            value={settings[SK.cacheRetention] ?? ""}
            onChange={(e) => set(SK.cacheRetention, e.target.value)}
          >
            <option value="">5 minutes (default)</option>
            <option value="long">1 hour (2× write price)</option>
          </Select>
        </Field>
      </Card>

      <Card>
        <div className="mb-2 text-sm text-gray-200">Per-server overrides</div>
        <div className="mb-3 text-xs text-gray-500">
          Leave on "Use default" unless a specific server needs a different policy
          (e.g. full autonomy on a sandbox, approve-each on production).
        </div>
        {vpsList.length === 0 && (
          <div className="text-xs text-gray-600">No servers yet.</div>
        )}
        <div className="space-y-2">
          {vpsList.map((v) => {
            const key = `${SK.safetyMode}.${v.id}`;
            const value = settings[key] ?? "";
            return (
              <div key={v.id} className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-sm text-gray-300">
                  {v.name}
                </span>
                <Select
                  value={value}
                  onChange={(e) => set(key, e.target.value)}
                  className="w-44"
                >
                  <option value="">Use default</option>
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
      </Card>
    </div>
  );
}
