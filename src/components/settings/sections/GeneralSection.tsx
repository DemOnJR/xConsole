import { useSettingsStore } from "../../../stores/settingsStore";
import { Card, Field, SectionHeader, Select, Toggle } from "../ui";

export const SK = {
  agentEnabled: "agent.enabled",
  activeProvider: "agent.active_provider",
  safetyMode: "agent.safety_mode",
  contextTokens: "agent.context_tokens",
} as const;

export function GeneralSection() {
  const settings = useSettingsStore((s) => s.settings);
  const providers = useSettingsStore((s) => s.providers);
  const set = useSettingsStore((s) => s.set);

  const agentEnabled = settings[SK.agentEnabled] !== "false";
  const activeProvider = settings[SK.activeProvider] ?? "";

  return (
    <div>
      <SectionHeader
        title="General"
        description="Core agent settings. xConsole is built to grow into a full DevOps cockpit; this is the brain's master switch."
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
    </div>
  );
}
