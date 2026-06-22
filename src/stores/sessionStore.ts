import { create } from "zustand";

export type ConnState =
  | "connecting"
  | "connected"
  | "reconnecting"
  | "disconnected"
  | "error";

export interface SessionInfo {
  sessionId?: string;
  status: ConnState;
  hostKey?: string;
  error?: string;
  /** Remote working directory when known (OSC 7 / cd tracking). */
  cwd?: string;
}

interface SessionState {
  /** Keyed by canvas node id. */
  sessions: Record<string, SessionInfo>;
  setInfo: (nodeId: string, partial: Partial<SessionInfo>) => void;
  remove: (nodeId: string) => void;
}

export const useSessionStore = create<SessionState>((set) => ({
  sessions: {},
  setInfo: (nodeId, partial) =>
    set((s) => {
      const prev: SessionInfo = s.sessions[nodeId] ?? { status: "connecting" };
      return {
        sessions: { ...s.sessions, [nodeId]: { ...prev, ...partial } },
      };
    }),
  remove: (nodeId) =>
    set((s) => {
      const next = { ...s.sessions };
      delete next[nodeId];
      return { sessions: next };
    }),
}));
