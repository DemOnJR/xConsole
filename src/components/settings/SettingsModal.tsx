import { useEffect, type ComponentType } from "react";
import { useUiStore } from "../../stores/uiStore";
import { useSettingsStore } from "../../stores/settingsStore";
import {
  BotIcon,
  BrainIcon,
  PaletteIcon,
  PlugIcon,
  SettingsIcon,
  ShieldIcon,
  SparkIcon,
  CloseIcon,
  ToolsIcon,
} from "../icons";
import { GeneralSection } from "./sections/GeneralSection";
import { ThemeSection } from "./sections/ThemeSection";
import { ModelsSection } from "./sections/ModelsSection";
import { ProvidersSection } from "./sections/ProvidersSection";
import { AgentSection } from "./sections/AgentSection";
import { KnowledgeSection } from "./sections/KnowledgeSection";
import { SecuritySection } from "./sections/SecuritySection";
import { PluginsSection } from "./sections/PluginsSection";
import { AdvancedSection } from "./sections/AdvancedSection";

interface Category {
  id: string;
  label: string;
  icon: ComponentType<{ size?: number; className?: string }>;
  Component: ComponentType;
  group: "core" | "ai" | "system";
}

const CATEGORIES: Category[] = [
  { id: "general", label: "General", icon: SettingsIcon, Component: GeneralSection, group: "core" },
  { id: "theme", label: "Appearance", icon: PaletteIcon, Component: ThemeSection, group: "core" },
  { id: "plugins", label: "Plugins & Harness", icon: PlugIcon, Component: PluginsSection, group: "core" },
  { id: "providers", label: "AI Providers", icon: PlugIcon, Component: ProvidersSection, group: "ai" },
  { id: "models", label: "Local Models", icon: BrainIcon, Component: ModelsSection, group: "ai" },
  { id: "agent", label: "Agent & Safety", icon: BotIcon, Component: AgentSection, group: "ai" },
  { id: "knowledge", label: "Knowledge Base", icon: SparkIcon, Component: KnowledgeSection, group: "ai" },
  { id: "security", label: "Security & Privacy", icon: ShieldIcon, Component: SecuritySection, group: "system" },
  { id: "advanced", label: "Advanced Tools", icon: ToolsIcon, Component: AdvancedSection, group: "system" },
];

const LEGACY_SECTION: Record<string, string> = {
  voice: "advanced",
  hooks: "advanced",
  cron: "advanced",
  cloud: "advanced",
  projects: "advanced",
  artifacts: "advanced",
  soul: "knowledge",
  memory: "knowledge",
  skills: "knowledge",
};

const GROUP_LABEL: Record<string, string> = {
  core: "Application",
  ai: "AI & Engine",
  system: "System & Tools",
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

  const groups = (["core", "ai", "system"] as const).map((g) => ({
    id: g,
    label: GROUP_LABEL[g],
    items: CATEGORIES.filter((c) => c.group === g),
  }));

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4 sm:p-6 select-none"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="flex h-[min(84vh,740px)] w-[min(960px,95vw)] overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface-2)] shadow-2xl">
        {/* Navigation sidebar */}
        <nav className="flex w-52 shrink-0 flex-col border-r border-[var(--border)] bg-[var(--bg)]/90 py-3">
          <div className="px-4 pb-3 flex items-center gap-2 border-b border-[var(--border)]/60">
            <div className="flex h-6 w-6 items-center justify-center rounded bg-cyan-500/10 text-cyan-400">
              <SettingsIcon size={14} />
            </div>
            <span className="text-xs font-semibold uppercase tracking-wider text-gray-200">
              Preferences
            </span>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-2 pt-3 space-y-4">
            {groups.map((g) => (
              <div key={g.id}>
                <div className="px-2.5 pb-1 text-[10px] font-mono font-semibold uppercase tracking-wider text-gray-500">
                  {g.label}
                </div>
                <div className="space-y-0.5">
                  {g.items.map((c) => {
                    const Icon = c.icon;
                    const isActive = c.id === active.id;
                    return (
                      <button
                        key={c.id}
                        type="button"
                        onClick={() => setSection(c.id)}
                        className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-xs font-medium transition ${
                          isActive
                            ? "bg-cyan-500/15 text-cyan-400 border border-cyan-500/30"
                            : "text-gray-400 hover:bg-[var(--surface-hover)] hover:text-gray-100 border border-transparent"
                        }`}
                      >
                        <Icon size={14} className={isActive ? "text-cyan-400" : "text-gray-400"} />
                        <span>{c.label}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        </nav>

        {/* Content View */}
        <div className="flex min-w-0 flex-1 flex-col bg-[var(--surface)]">
          <header className="flex items-center justify-between border-b border-[var(--border)] px-6 py-3 bg-[var(--surface-2)]/60">
            <div className="flex items-center gap-2 text-xs font-medium text-gray-300">
              <span className="text-gray-500">Settings</span>
              <span className="text-gray-600">/</span>
              <span className="text-gray-100 font-semibold">{active.label}</span>
            </div>
            <button
              type="button"
              onClick={close}
              className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-gray-400 transition hover:bg-[var(--border)] hover:text-white"
              title="Close settings (Esc)"
            >
              <span className="text-[10px] font-mono bg-zinc-800 border border-zinc-700 px-1 py-0.2 rounded text-gray-400">Esc</span>
              <CloseIcon size={12} />
            </button>
          </header>

          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
            <Active />
          </div>
        </div>
      </div>
    </div>
  );
}
