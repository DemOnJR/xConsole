import { create } from "zustand";
import type { PersonaLiveStatus } from "../lib/tauri";

export const PHASE_META: Record<string, { label: string; color: string }> = {
  idle: { label: "Idle", color: "#6b7280" },
  thinking: { label: "Thinking", color: "#a855f7" },
  planning: { label: "Planning", color: "#a855f7" },
  working: { label: "Working", color: "#3b82f6" },
  waiting: { label: "Waiting", color: "#eab308" },
  verifying: { label: "Verifying", color: "#22c55e" },
  testing: { label: "Verifying", color: "#22c55e" },
  blocked: { label: "Blocked", color: "#ef4444" },
};

export type PersonaStatusEntry = PersonaLiveStatus & { updatedAt: number };

interface PersonaStatusState {
  byKey: Record<string, PersonaStatusEntry>;
  ingest: (s: PersonaLiveStatus) => void;
}

export function personaStatusKey(s: Pick<PersonaLiveStatus, "persona_id" | "session_id">): string {
  return s.persona_id || `session:${s.session_id}`;
}

export const usePersonaStatusStore = create<PersonaStatusState>((set) => ({
  byKey: {},
  ingest: (s) =>
    set((st) => {
      const next = { ...st.byKey };
      const k = personaStatusKey(s);
      if (s.status === "idle") {
        delete next[k];
      } else {
        next[k] = { ...s, updatedAt: Date.now() };
      }
      return { byKey: next };
    }),
}));
