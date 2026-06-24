import { create } from "zustand";
import { api, type FileChange } from "../lib/tauri";

// Tracks the files the agent edited in the current chat session, for the changes
// (diff) panel. Populated from the backend edit journal: an initial fetch per
// session plus live `agent://file-change` events (wired in App).

interface EditsState {
  /** The chat session whose changes we're showing. */
  sessionId: string | null;
  changes: FileChange[];
  open: boolean;
  selectedId: string | null;
  reverting: string | null;

  setOpen: (v: boolean) => void;
  toggle: () => void;
  select: (id: string | null) => void;
  /** Switch to a session and load its recorded changes. */
  sync: (sessionId: string | null) => Promise<void>;
  /** Append a change from a live event (if it belongs to the shown session). */
  ingest: (c: FileChange) => void;
  markReverted: (id: string) => void;
  revert: (id: string) => Promise<void>;
}

export const useEditsStore = create<EditsState>((set, get) => ({
  sessionId: null,
  changes: [],
  open: false,
  selectedId: null,
  reverting: null,

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
}));
