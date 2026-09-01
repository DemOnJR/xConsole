import type { GoalSession, Persona } from "../../../src/lib/tauri";
import {
  PHASE_META,
  type PersonaStatusEntry,
} from "../../../src/stores/personaStatusStore";

const LIVE_TTL_MS = 120_000;

const RUNNING_GOAL = new Set(["active", "intake", "waiting", "blocked"]);

export function goalPhase(status: string): string {
  switch (status) {
    case "active":
      return "working";
    case "intake":
      return "planning";
    case "waiting":
      return "waiting";
    case "blocked":
      return "blocked";
    default:
      return "idle";
  }
}

export function runningGoal(personaId: string, goals: GoalSession[]): GoalSession | null {
  const running = goals
    .filter((g) => g.persona_id === personaId && RUNNING_GOAL.has(g.status))
    .sort((a, b) => (b.updated_at || "").localeCompare(a.updated_at || ""));
  return running[0] ?? null;
}

export function memberLive(
  persona: Persona,
  live: Record<string, PersonaStatusEntry>,
  goals: GoalSession[],
): { phase: string; label: string; task: string | null } {
  const hit = live[persona.id];
  const task = runningGoal(persona.id, goals)?.title ?? null;
  if (hit && Date.now() - hit.updatedAt < LIVE_TTL_MS) {
    const meta = PHASE_META[hit.status] || PHASE_META.working;
    const label = (hit.detail || "").trim() || meta.label;
    return { phase: hit.status, label, task };
  }
  const goal = runningGoal(persona.id, goals);
  if (!goal) return { phase: "idle", label: "Idle", task: null };
  const phase = goalPhase(goal.status);
  const meta = PHASE_META[phase] || PHASE_META.idle;
  return { phase, label: meta.label, task: goal.title };
}

export function phaseColor(phase: string): string {
  return (PHASE_META[phase] || PHASE_META.idle).color;
}
