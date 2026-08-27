import { useState, type ComponentType } from "react";
import { SectionHeader } from "../ui";
import { VoiceSection } from "./VoiceSection";
import { HooksSection } from "./HooksSection";
import { CronSection } from "./CronSection";
import { CloudSection } from "./CloudSection";
import { ProjectsSection } from "./ProjectsSection";

interface AdvancedTab {
  id: string;
  label: string;
  description: string;
  Component: ComponentType;
}

const TABS: AdvancedTab[] = [
  {
    id: "voice",
    label: "Voice (STT/TTS)",
    description: "Hands-free speech-to-text recognition and text-to-speech voice engines.",
    Component: VoiceSection,
  },
  {
    id: "cron",
    label: "Scheduled Cron",
    description: "Automate recurring maintenance tasks, heartbeats, and scheduled agent jobs.",
    Component: CronSection,
  },
  {
    id: "hooks",
    label: "Lifecycle Hooks",
    description: "Execute shell scripts or triggers on agent events (prompt submit, tool use, completion).",
    Component: HooksSection,
  },
  {
    id: "cloud",
    label: "Cloud Credentials",
    description: "Cloudflare, AWS, GCP, and Terraform Cloud accounts for infrastructure tools.",
    Component: CloudSection,
  },
  {
    id: "projects",
    label: "IaC Projects",
    description: "Terraform roots and local infrastructure project definitions.",
    Component: ProjectsSection,
  },
];

export function AdvancedSection() {
  const [activeTabId, setActiveTabId] = useState<string>("voice");
  const activeTab = TABS.find((t) => t.id === activeTabId) ?? TABS[0];
  const ActiveComponent = activeTab.Component;

  return (
    <div className="space-y-5">
      <SectionHeader
        title="Advanced Tools & Automation"
        description="Power-user utilities: Voice STT/TTS, recurring cron jobs, lifecycle hooks, and cloud infrastructure credentials."
      />

      <div className="flex flex-wrap gap-1 rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-1">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setActiveTabId(t.id)}
            className={`flex-1 min-w-[120px] rounded-md px-2.5 py-1.5 text-xs font-medium transition ${
              t.id === activeTab.id
                ? "bg-cyan-500/20 text-cyan-300 shadow-xs border border-cyan-500/30"
                : "text-gray-400 hover:text-gray-200 border border-transparent"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="pt-1">
        <ActiveComponent />
      </div>
    </div>
  );
}
