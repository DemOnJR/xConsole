import { create } from "zustand";
import { api, type FileChange } from "../lib/tauri";

// Tracks the files the agent edited, for the changes (diff) panel. Two views:
//   * live: the current chat session's changes (in-memory + DB, live events)
//   * history: everything the DB remembers, filterable by workspace/session.

export interface ChangeGroup {
  sessionId: string;
  workspaceId: string | null;
  changes: FileChange[];
}

interface EditsState {
  /** The chat session whose changes we're showing in live mode. */
  sessionId: string | null;
  changes: FileChange[];
  open: boolean;
  selectedId: string | null;
  reverting: string | null;
  /** history mode */
  mode: "live" | "history";
  historyGroups: ChangeGroup[];
  historyWorkspace: string | null;
  historySession: string | null;

  setOpen: (v: boolean) => void;
  toggle: () => void;
  select: (id: string | null) => void;
  /** Switch to a session and load its recorded changes. */
  sync: (sessionId: string | null) => Promise<void>;
  /** Append a change from a live event (if it belongs to the shown session). */
  ingest: (c: FileChange) => void;
  markReverted: (id: string) => void;
  revert: (id: string) => Promise<void>;
  setMode: (mode: "live" | "history") => void;
  setHistoryFilters: (workspace: string | null, session: string | null) => void;
  loadHistory: () => Promise<void>;
}

const groupBy = (changes: FileChange[]): ChangeGroup[] => {
  const map = new Map<string, ChangeGroup>();
  for (const c of changes) {
    const key = `${c.session_id}::${c.workspace_id ?? ""}`;
    let g = map.get(key);
    if (!g) {
      g = { sessionId: c.session_id, workspaceId: c.workspace_id ?? null, changes: [] };
      map.set(key, g);
    }
    g.changes.push(c);
  }
  return [...map.values()].sort(
    (a, b) =>
      (b.changes[b.changes.length - 1]?.ts ?? 0) - (a.changes[a.changes.length - 1]?.ts ?? 0),
  );
};

export const useEditsStore = create<EditsState>((set, get) => ({
  sessionId: null,
  changes: [],
  open: false,
  selectedId: null,
  reverting: null,
  mode: "live",
  historyGroups: [],
  historyWorkspace: null,
  historySession: null,

  setOpen: (open) => set({ open }),
  toggle: () => set((s) => ({ open: !s.open })),
  select: (selectedId) => set({ selectedId }),

  sync: async (sessionId) => {
    set({ sessionId, changes: [], selectedId: null });
    if (!sessionId) return;
    try {
      const changes = await api.listFileChanges(sessionId);
      // Guard against a race where the session changed while we awaited.
      if (get().sessionId !== sessionId) return;
      set({ changes, selectedId: changes.length ? changes[changes.length - 1].id : null });
    } catch {
      /* ignore */
    }
  },

  ingest: (c) => {
    if (c.session_id !== get().sessionId) return;
    set((s) => {
      if (s.changes.some((x) => x.id === c.id)) return s;
      return { changes: [...s.changes, c], selectedId: c.id };
    });
  },

  markReverted: (id) =>
    set((s) => ({
      changes: s.changes.map((c) => (c.id === id ? { ...c, reverted: true } : c)),
      historyGroups: s.historyGroups.map((g) => ({
        ...g,
        changes: g.changes.map((c) => (c.id === id ? { ...c, reverted: true } : c)),
      })),
    })),

  revert: async (id) => {
    set({ reverting: id });
    try {
      await api.revertFileChange(id);
      get().markReverted(id);
    } catch {
      /* surfaced via the disabled state resetting */
    } finally {
      set({ reverting: null });
    }
  },

  setMode: (mode) => set({ mode, selectedId: null }),

  setHistoryFilters: (workspace, session) =>
    set({ historyWorkspace: workspace, historySession: session }),

  loadHistory: async () => {
    const { historyWorkspace, historySession } = get();
    try {
      const changes = await api.listFileChangesHistory(historyWorkspace, historySession);
      const groups = groupBy(changes);
      const selectedId =
        groups[0]?.changes[groups[0].changes.length - 1]?.id ?? null;
      set({ historyGroups: groups, selectedId });
    } catch {
      /* ignore */
    }
  },
}));
