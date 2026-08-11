import { useState, type ComponentType, type ReactNode } from "react";
import { SectionHeader } from "../ui";
import { VoiceSection } from "./VoiceSection";
import { HooksSection } from "./HooksSection";
import { CronSection } from "./CronSection";
import { CloudSection } from "./CloudSection";
import { ProjectsSection } from "./ProjectsSection";

interface AdvancedBlock {
  id: string;
  label: string;
  hint: string;
  Component: ComponentType;
}

const BLOCKS: AdvancedBlock[] = [
  {
    id: "voice",
    label: "Voice",
    hint: "Speech-to-text and text-to-speech engines for hands-free agent turns.",
    Component: VoiceSection,
  },
  {
    id: "hooks",
    label: "Hooks",
    hint: "Run scripts on agent lifecycle events (prompt submit, tool use, stop).",
    Component: HooksSection,
  },
  {
    id: "cron",
    label: "Cron",
    hint: "Schedule recurring agent jobs or remote commands.",
    Component: CronSection,
  },
  {
    id: "cloud",
    label: "Cloud accounts",
    hint: "AWS / GCP credentials for infrastructure tools.",
    Component: CloudSection,
  },
  {
    id: "projects",
    label: "Terraform projects",
    hint: "IaC project roots the agent can load and apply skills against.",
    Component: ProjectsSection,
  },
];

function Fold({
  label,
  hint,
  open,
  onToggle,
  children,
}: {
  label: string;
  hint: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className="mb-2 overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--bg)]">
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full items-start gap-3 px-3 py-2.5 text-left transition hover:bg-[var(--surface-hover)]"
      >
        <span
          className="mt-0.5 text-[var(--text-faint)] transition-transform"
          style={{ transform: open ? "rotate(90deg)" : "none" }}
          aria-hidden
        >
          ▸
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-sm text-[var(--text)]">{label}</span>
          <span className="mt-0.5 block text-[11px] leading-relaxed text-[var(--text-faint)]">
            {hint}
          </span>
        </span>
      </button>
      {open ? (
        <div className="border-t border-[var(--border)] px-3 py-3">{children}</div>
      ) : null}
    </div>
  );
}

/**
 * Power-user features kept out of the top-level settings nav so the
 * primary surface stays compact (Voice, Hooks, Cron, Cloud, Terraform).
 */
export function AdvancedSection() {
  const [openId, setOpenId] = useState<string | null>(null);

  return (
    <div>
      <SectionHeader
        title="Advanced"
        description="Optional power features. Everything here works; most daily DevOps workflows never need these open."
      />
      {BLOCKS.map((b) => {
        const Comp = b.Component;
        const open = openId === b.id;
        return (
          <Fold
            key={b.id}
            label={b.label}
            hint={b.hint}
            open={open}
            onToggle={() => setOpenId(open ? null : b.id)}
          >
            {/* Nested sections already render their own headers — keep content only. */}
            <div className="[&>div>div:first-child]:mb-3">
              <Comp />
            </div>
          </Fold>
        );
      })}
    </div>
  );
}
