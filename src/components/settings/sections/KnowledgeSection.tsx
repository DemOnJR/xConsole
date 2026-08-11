import { useState, type ComponentType } from "react";
import { SectionHeader } from "../ui";
import { SoulSection } from "./SoulSection";
import { MemorySection } from "./MemorySection";
import { SkillsSection } from "./SkillsSection";

const TABS: { id: string; label: string; Component: ComponentType }[] = [
  { id: "soul", label: "Soul", Component: SoulSection },
  { id: "memory", label: "Memory", Component: MemorySection },
  { id: "skills", label: "Skills", Component: SkillsSection },
];

/**
 * Combines Soul / Memory / Skills into one settings category so the
 * sidebar stays short without losing any of those editors.
 */
export function KnowledgeSection() {
  const [tab, setTab] = useState("memory");
  const active = TABS.find((t) => t.id === tab) ?? TABS[0];
  const Active = active.Component;

  return (
    <div>
      <SectionHeader
        title="Knowledge"
        description="What the agent always knows: identity (soul), durable facts (memory), and reusable playbooks (skills)."
      />
      <div className="mb-4 flex gap-1 rounded-lg border border-[var(--border)] bg-[var(--bg)] p-1">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`flex-1 rounded-md px-2 py-1.5 text-xs font-medium transition ${
              t.id === active.id
                ? "bg-[var(--accent-muted)] text-[var(--accent)]"
                : "text-[var(--text-faint)] hover:text-[var(--text)]"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      {/* Hide the nested section's own title block to avoid double headers. */}
      <div className="[&_h2]:hidden [&_.mb-4.flex.items-start]:mb-2">
        <Active />
      </div>
    </div>
  );
}
