import { useState, type ComponentType } from "react";
import { SectionHeader } from "../ui";
import { SoulSection } from "./SoulSection";
import { MemorySection } from "./MemorySection";
import { SkillsSection } from "./SkillsSection";

const TABS: { id: string; label: string; Component: ComponentType }[] = [
  { id: "memory", label: "Persistent Memory (MEMORY.md)", Component: MemorySection },
  { id: "soul", label: "Agent Identity (SOUL.md)", Component: SoulSection },
  { id: "skills", label: "Playbooks & Skills", Component: SkillsSection },
];

export function KnowledgeSection() {
  const [tab, setTab] = useState("memory");
  const active = TABS.find((t) => t.id === tab) ?? TABS[0];
  const Active = active.Component;

  return (
    <div className="space-y-5">
      <SectionHeader
        title="Knowledge Base & Context"
        description="Persistent facts (Memory), behavioral identity (Soul), and reusable operational playbooks (Skills)."
      />

      <div className="flex gap-1 rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-1">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`flex-1 rounded-md px-3 py-1.5 text-xs font-medium transition ${
              t.id === active.id
                ? "bg-cyan-500/20 text-cyan-300 shadow-xs border border-cyan-500/30"
                : "text-gray-400 hover:text-gray-200 border border-transparent"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="pt-1">
        <Active />
      </div>
    </div>
  );
}
