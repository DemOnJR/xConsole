import { useEffect, type ComponentType } from "react";
import { useUiStore } from "../../stores/uiStore";
import { useSettingsStore } from "../../stores/settingsStore";
import {
  BotIcon,
  BookIcon,
  BrainIcon,
  ClockIcon,
  GridIcon,
  PaletteIcon,
  PlugIcon,
  SettingsIcon,
  ShieldIcon,
  SparkIcon,
} from "../icons";
import { GeneralSection } from "./sections/GeneralSection";
import { ThemeSection } from "./sections/ThemeSection";
import { ModelsSection } from "./sections/ModelsSection";
import { VoiceSection } from "./sections/VoiceSection";
import { ProvidersSection } from "./sections/ProvidersSection";
import { AgentSection } from "./sections/AgentSection";
import { SoulSection } from "./sections/SoulSection";
import { MemorySection } from "./sections/MemorySection";
import { SkillsSection } from "./sections/SkillsSection";
import { CloudSection } from "./sections/CloudSection";
import { CronSection } from "./sections/CronSection";
import { ProjectsSection } from "./sections/ProjectsSection";
import { SecuritySection } from "./sections/SecuritySection";

interface Category {
  id: string;
  label: string;
  icon: ComponentType<{ size?: number }>;
  Component: ComponentType;
}

/** Single source of truth for the settings categories. Add a category here. */
const CATEGORIES: Category[] = [
  { id: "general", label: "General", icon: SettingsIcon, Component: GeneralSection },
  { id: "theme", label: "Theme", icon: PaletteIcon, Component: ThemeSection },
  { id: "providers", label: "Providers", icon: PlugIcon, Component: ProvidersSection },
  { id: "models", label: "Models", icon: BrainIcon, Component: ModelsSection },
  { id: "voice", label: "Voice", icon: SparkIcon, Component: VoiceSection },
  { id: "agent", label: "Agent & Safety", icon: BotIcon, Component: AgentSection },
  { id: "soul", label: "Soul", icon: SparkIcon, Component: SoulSection },
  { id: "memory", label: "Memory", icon: BrainIcon, Component: MemorySection },
  { id: "skills", label: "Skills", icon: BookIcon, Component: SkillsSection },
  { id: "projects", label: "Projects", icon: GridIcon, Component: ProjectsSection },
  { id: "cloud", label: "Cloud", icon: PlugIcon, Component: CloudSection },
  { id: "cron", label: "Cron", icon: ClockIcon, Component: CronSection },
  { id: "security", label: "Security", icon: ShieldIcon, Component: SecuritySection },
];

export function SettingsModal() {
  const open = useUiStore((s) => s.settingsOpen);
  const section = useUiStore((s) => s.settingsSection);
  const setSection = useUiStore((s) => s.setSettingsSection);
  const close = useUiStore((s) => s.closeSettings);
  const loadSettings = useSettingsStore((s) => s.load);

  useEffect(() => {
    if (open) loadSettings();
  }, [open, loadSettings]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && open) close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  if (!open) return null;

  const active = CATEGORIES.find((c) => c.id === section) ?? CATEGORIES[0];
  const Active = active.Component;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="flex h-[80vh] w-[min(960px,92vw)] overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface-2)] shadow-2xl">
        {/* Category sidebar */}
        <nav className="flex w-52 shrink-0 flex-col border-r border-[var(--border)] bg-[var(--bg)] py-3">
          <div className="px-4 pb-3 text-xs font-semibold uppercase tracking-wider text-gray-500">
            Settings
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-2">
            {CATEGORIES.map((c) => {
              const Icon = c.icon;
              const isActive = c.id === active.id;
              return (
                <button
                  key={c.id}
                  onClick={() => setSection(c.id)}
                  className={`mb-0.5 flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition ${
                    isActive
                      ? "bg-blue-600/20 text-blue-100"
                      : "text-gray-400 hover:bg-[var(--surface)] hover:text-gray-200"
                  }`}
                >
                  <Icon size={16} />
                  {c.label}
                </button>
              );
            })}
          </div>
        </nav>

        {/* Active section */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-3">
            <span className="text-sm font-medium text-gray-200">{active.label}</span>
            <button
              onClick={close}
              className="rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
              title="Close (Esc)"
            >
              ✕
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
