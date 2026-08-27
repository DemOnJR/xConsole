import { useEffect, type ComponentType } from "react";
import { useUiStore } from "../../stores/uiStore";
import { useSettingsStore } from "../../stores/settingsStore";
import {
  BotIcon,
  BrainIcon,
  FolderIcon,
  PaletteIcon,
  PlugIcon,
  SettingsIcon,
  ShieldIcon,
  SparkIcon,
} from "../icons";
import { GeneralSection } from "./sections/GeneralSection";
import { ThemeSection } from "./sections/ThemeSection";
import { ModelsSection } from "./sections/ModelsSection";
import { ProvidersSection } from "./sections/ProvidersSection";
import { AgentSection } from "./sections/AgentSection";
import { KnowledgeSection } from "./sections/KnowledgeSection";
import { ArtifactsSection } from "./sections/ArtifactsSection";
import { SecuritySection } from "./sections/SecuritySection";
import { PluginsSection } from "./sections/PluginsSection";
import { AdvancedSection } from "./sections/AdvancedSection";

interface Category {
  id: string;
  label: string;
  icon: ComponentType<{ size?: number }>;
  Component: ComponentType;
  group?: "core" | "ai" | "more";
}

/**
 * Compact settings IA. Voice / Cron / Cloud / Terraform / Hooks live under Advanced.
 * Soul + Memory + Skills share Knowledge.
 */
const CATEGORIES: Category[] = [
  { id: "general", label: "General", icon: SettingsIcon, Component: GeneralSection, group: "core" },
  { id: "theme", label: "Appearance", icon: PaletteIcon, Component: ThemeSection, group: "core" },
  { id: "plugins", label: "Plugins & Harness", icon: PlugIcon, Component: PluginsSection, group: "core" },
  { id: "providers", label: "Providers", icon: PlugIcon, Component: ProvidersSection, group: "ai" },
  { id: "models", label: "Models", icon: BrainIcon, Component: ModelsSection, group: "ai" },
  { id: "agent", label: "Agent & Safety", icon: BotIcon, Component: AgentSection, group: "ai" },
  { id: "knowledge", label: "Knowledge", icon: SparkIcon, Component: KnowledgeSection, group: "ai" },
  { id: "artifacts", label: "Artifacts", icon: FolderIcon, Component: ArtifactsSection, group: "ai" },
  { id: "security", label: "Security", icon: ShieldIcon, Component: SecuritySection, group: "more" },
  { id: "advanced", label: "Advanced", icon: SettingsIcon, Component: AdvancedSection, group: "more" },
];

/** Map old persisted section ids (pre-reorg) onto the new categories. */
const LEGACY_SECTION: Record<string, string> = {
  voice: "advanced",
  hooks: "advanced",
  cron: "advanced",
  cloud: "advanced",
  projects: "advanced",
  soul: "knowledge",
  memory: "knowledge",
  skills: "knowledge",
};

const GROUP_LABEL: Record<string, string> = {
  core: "App",
  ai: "AI",
  more: "More",
};

export function SettingsModal() {
  const open = useUiStore((s) => s.settingsOpen);
  const section = useUiStore((s) => s.settingsSection);
  const setSection = useUiStore((s) => s.setSettingsSection);
  const close = useUiStore((s) => s.closeSettings);
  const loadSettings = useSettingsStore((s) => s.load);

  const resolvedSection = LEGACY_SECTION[section] ?? section;

  useEffect(() => {
    if (open) loadSettings();
  }, [open, loadSettings]);

  useEffect(() => {
    if (LEGACY_SECTION[section]) {
      setSection(LEGACY_SECTION[section]);
    }
  }, [section, setSection]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && open) close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  if (!open) return null;

  const active =
    CATEGORIES.find((c) => c.id === resolvedSection) ?? CATEGORIES[0];
  const Active = active.Component;

  const groups = (["core", "ai", "more"] as const).map((g) => ({
    id: g,
    label: GROUP_LABEL[g],
    items: CATEGORIES.filter((c) => c.group === g),
  }));

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="flex h-[min(80vh,720px)] w-[min(920px,94vw)] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface-2)] shadow-[var(--shadow-panel)]">
        {/* Category sidebar */}
        <nav className="flex w-48 shrink-0 flex-col border-r border-[var(--border)] bg-[var(--bg)] py-3">
          <div className="px-4 pb-3 text-xs font-semibold uppercase tracking-wider text-[var(--text-faint)]">
            Settings
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-2">
            {groups.map((g) => (
              <div key={g.id} className="mb-3">
                <div className="px-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                  {g.label}
                </div>
                {g.items.map((c) => {
                  const Icon = c.icon;
                  const isActive = c.id === active.id;
                  return (
                    <button
                      key={c.id}
                      onClick={() => setSection(c.id)}
                      className={`mb-0.5 flex w-full items-center gap-2.5 rounded-[var(--radius-md)] px-2.5 py-2 text-left text-sm transition ${
                        isActive
                          ? "bg-[var(--accent-muted)] text-[var(--accent)]"
                          : "text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
                      }`}
                    >
                      <Icon size={15} />
                      {c.label}
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        </nav>

        {/* Active section */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-3">
            <span className="text-sm font-medium text-[var(--text)]">{active.label}</span>
            <button
              onClick={close}
              className="rounded-[var(--radius-md)] px-2 py-1 text-xs text-[var(--text-faint)] transition hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
            >
              Esc
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
            <Active />
          </div>
        </div>
      </div>
    </div>
  );
}
