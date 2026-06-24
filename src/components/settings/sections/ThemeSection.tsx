import { useState } from "react";
import { useThemeStore } from "../../../stores/themeStore";
import { THEMES, type UiVars } from "../../../lib/themes";

/** The colors exposed in the custom-theme editor. */
const CUSTOM_FIELDS: { key: keyof UiVars; label: string }[] = [
  { key: "bg", label: "Background" },
  { key: "surface", label: "Surface" },
  { key: "border", label: "Border" },
  { key: "text", label: "Text" },
  { key: "accent", label: "Accent" },
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
      <div>
        <h3 className="mb-1 text-sm font-medium text-[var(--text)]">Theme</h3>
        <p className="mb-3 text-xs text-[var(--text-dim)]">
          Recolors the whole app and your terminals instantly.
        </p>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
          {THEMES.map((t) => {
            const active = themeId === t.id;
            return (
              <button
                key={t.id}
                onClick={() => void setTheme(t.id)}
                className={`flex flex-col gap-2 rounded-lg border p-2.5 text-left transition ${
                  active
                    ? "border-[var(--accent)] ring-1 ring-[var(--accent)]"
                    : "border-[var(--border)] hover:border-[var(--border-strong)]"
                }`}
                style={{ background: t.vars.bg }}
              >
                <div className="flex gap-1">
                  {[t.vars.surface, t.vars.border, t.vars.accent, t.vars.text].map(
                    (c, i) => (
                      <span
                        key={i}
                        className="h-4 w-4 rounded-full"
                        style={{ background: c }}
                      />
                    ),
                  )}
                </div>
                <span className="text-xs font-medium" style={{ color: t.vars.text }}>
                  {t.name}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="border-t border-[var(--border)] pt-4">
        <h3 className="mb-1 text-sm font-medium text-[var(--text)]">Custom theme</h3>
        <p className="mb-3 text-xs text-[var(--text-dim)]">
          Pick your own colors. Saving applies and persists a "Custom" theme.
        </p>
        <div className="space-y-2">
          {CUSTOM_FIELDS.map((f) => (
            <div key={f.key} className="flex items-center gap-3">
              <input
                type="color"
                value={custom[f.key]}
                onChange={(e) =>
                  setCustom((c) => ({ ...c, [f.key]: e.target.value }))
                }
                className="h-7 w-10 cursor-pointer rounded border border-[var(--border)] bg-transparent"
              />
              <span className="w-24 text-xs text-[var(--text-dim)]">{f.label}</span>
              <span className="font-mono text-[11px] text-[var(--text-faint)]">
                {custom[f.key]}
              </span>
            </div>
          ))}
        </div>
        <button
          onClick={() => void saveCustom(custom)}
          className="mt-3 rounded-md bg-[var(--accent)] px-3 py-1.5 text-xs font-medium text-[var(--accent-fg)] hover:opacity-90"
        >
          Apply custom theme
        </button>
      </div>
    </div>
  );
}
