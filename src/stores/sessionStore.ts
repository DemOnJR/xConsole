import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

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
  /** SFTP panels: the remote path currently being browsed. */
  sftpPath?: string;
  /** Git branch when cwd/sftpPath is inside a repo (`null` = not a repo / unknown). */
  gitBranch?: string | null;
  /** Uncommitted changes in that work tree. */
  gitDirty?: boolean;
}

interface SessionState {
  /** Keyed by canvas node id. */
  sessions: Record<string, SessionInfo>;
  setInfo: (nodeId: string, partial: Partial<SessionInfo>) => void;
  remove: (nodeId: string) => void;
  clear: () => void;
}

export const useSessionStore = create<SessionState>()(
  persist(
    (set) => ({
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
      clear: () => set({ sessions: {} }),
    }),
    {
      name: "xconsole-sessions",
      storage: createJSONStorage(() => ({
        getItem: (name) =>
          typeof localStorage !== "undefined" ? localStorage.getItem(name) : null,
        setItem: (name, value) => {
          if (typeof localStorage !== "undefined") localStorage.setItem(name, value);
        },
        removeItem: (name) => {
          if (typeof localStorage !== "undefined") localStorage.removeItem(name);
        },
      })),
    },
  ),
);
