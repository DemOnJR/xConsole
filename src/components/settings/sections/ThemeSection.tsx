import { useState } from "react";
import { useThemeStore } from "../../../stores/themeStore";
import { THEMES, type UiVars } from "../../../lib/themes";
import { SectionHeader, SettingsGroup, Button } from "../ui";
import { CheckIcon } from "../../icons";

/** The colors exposed in the custom-theme editor. */
const CUSTOM_FIELDS: { key: keyof UiVars; label: string }[] = [
  { key: "bg", label: "App Background" },
  { key: "surface", label: "Surface Background" },
  { key: "border", label: "Border Color" },
  { key: "text", label: "Primary Text" },
  { key: "accent", label: "Accent Highlight" },
];

export function ThemeSection() {
  const themeId = useThemeStore((s) => s.themeId);
  const setTheme = useThemeStore((s) => s.setTheme);
  const saveCustom = useThemeStore((s) => s.saveCustom);
  const current = useThemeStore((s) => s.current);

  const base = current();
  const [custom, setCustom] = useState<UiVars>(base.vars);

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Appearance & Theme"
        description="Choose a theme for the xConsole user interface and embedded SSH terminals."
      />

      <SettingsGroup title="Curated Themes">
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
          {THEMES.map((t) => {
            const active = themeId === t.id;
            return (
              <button
                key={t.id}
                type="button"
                onClick={() => void setTheme(t.id)}
                className={`relative flex flex-col justify-between rounded-lg border p-3 text-left transition-all ${
                  active
                    ? "border-cyan-500 bg-cyan-500/10 shadow-sm ring-1 ring-cyan-500/40"
                    : "border-[var(--border)] bg-[var(--surface-2)] hover:border-zinc-500 hover:bg-[var(--surface-hover)]"
                }`}
              >
                <div className="flex items-center justify-between mb-3">
                  <span className="text-xs font-semibold text-gray-200 truncate">
                    {t.name}
                  </span>
                  {active && (
                    <span className="flex h-4 w-4 items-center justify-center rounded-full bg-cyan-500 text-white shrink-0">
                      <CheckIcon size={10} />
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-1.5 rounded-md p-1.5 bg-black/40 border border-white/5">
                  <span
                    className="h-3.5 w-3.5 rounded-full border border-white/10 shrink-0"
                    style={{ background: t.vars.bg }}
                    title="Background"
                  />
                  <span
                    className="h-3.5 w-3.5 rounded-full border border-white/10 shrink-0"
                    style={{ background: t.vars.surface }}
                    title="Surface"
                  />
                  <span
                    className="h-3.5 w-3.5 rounded-full border border-white/10 shrink-0"
                    style={{ background: t.vars.accent }}
                    title="Accent"
                  />
                  <span
                    className="h-3.5 w-3.5 rounded-full border border-white/10 shrink-0"
                    style={{ background: t.vars.text }}
                    title="Text"
                  />
                </div>
              </button>
            );
          })}
        </div>
      </SettingsGroup>

      <SettingsGroup
        title="Custom Palette Tuning"
        description="Fine-tune your own color palette. Saving persists and applies it as your custom theme."
        className="pt-4 border-t border-[var(--border)]"
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
          {CUSTOM_FIELDS.map((f) => (
            <div
              key={f.key}
              className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--surface)] p-2.5"
            >
              <span className="text-xs text-gray-300 font-medium">{f.label}</span>
              <div className="flex items-center gap-2">
                <input
                  type="color"
                  value={custom[f.key]}
                  onChange={(e) =>
                    setCustom((c) => ({ ...c, [f.key]: e.target.value }))
                  }
                  className="h-6 w-8 cursor-pointer rounded border border-[var(--border)] bg-transparent"
                />
                <span className="font-mono text-[11px] text-gray-400">
                  {custom[f.key]}
                </span>
              </div>
            </div>
          ))}
        </div>

        <div className="flex justify-end pt-2">
          <Button variant="primary" onClick={() => void saveCustom(custom)}>
            Apply Custom Colors
          </Button>
        </div>
      </SettingsGroup>
    </div>
  );
}
