import { create } from "zustand";
import { persist } from "zustand/middleware";
import {
  api,
  onTransferProgress,
  type ArchiveFormat,
  type TransferSnapshot,
} from "../lib/tauri";

/** Files moved at once. Beyond ~8 the SSH window, not the channel count, is the limit. */
export const CONCURRENCY_CHOICES = [1, 2, 4, 6, 8, 12, 16] as const;

interface TransferState {
  /** Live jobs keyed by id. The backend sends whole snapshots, so this is a mirror. */
  jobs: Record<string, TransferSnapshot>;
  /** Panel visibility. */
  open: boolean;
  /** How many files to move at once (persisted). */
  concurrency: number;
  /** Remembered download folder, so repeat downloads don't re-prompt (persisted). */
  lastDestination: string | null;
  subscribed: boolean;

  setOpen: (open: boolean) => void;
  setConcurrency: (n: number) => void;
  setLastDestination: (path: string | null) => void;

  /** Start listening for progress and pull any jobs already running. */
  subscribe: () => Promise<() => void>;
  ingest: (snapshot: TransferSnapshot) => void;
  refresh: () => Promise<void>;

  download: (sessionId: string, remotePaths: string[]) => Promise<void>;
  downloadArchive: (
    sessionId: string,
    remoteDir: string,
    format: ArchiveFormat,
  ) => Promise<void>;
  upload: (sessionId: string, remoteDir: string, localPaths?: string[]) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  clearFinished: () => Promise<void>;
}

/** Ask for a download folder, reusing the last one unless the user wants a new one. */
async function resolveDestination(
  get: () => TransferState,
  set: (partial: Partial<TransferState>) => void,
  force: boolean,
): Promise<string | null> {
  const remembered = get().lastDestination;
  if (remembered && !force) return remembered;
  const picked = await api.pickDirectory("Where should the files go?");
  if (picked) set({ lastDestination: picked });
  return picked;
}

export const useTransferStore = create<TransferState>()(
  persist(
    (set, get) => ({
      jobs: {},
      open: false,
      concurrency: 4,
      lastDestination: null,
      subscribed: false,

      setOpen: (open) => set({ open }),
      setConcurrency: (n) => set({ concurrency: n }),
      setLastDestination: (path) => set({ lastDestination: path }),

      subscribe: async () => {
        const un = await onTransferProgress((snapshot) => get().ingest(snapshot));
        set({ subscribed: true });
        await get().refresh();
        return () => {
          un();
          set({ subscribed: false });
        };
      },

      ingest: (snapshot) => {
        set((s) => ({ jobs: { ...s.jobs, [snapshot.id]: snapshot } }));
        // Surface the panel the moment work starts, so a transfer is never invisible.
        if (snapshot.state === "scanning" || snapshot.state === "running") {
          if (!get().open) set({ open: true });
        }
      },

      refresh: async () => {
        const list = await api.sftpTransferList();
        const jobs: Record<string, TransferSnapshot> = {};
        for (const j of list) jobs[j.id] = j;
        set({ jobs });
      },

      download: async (sessionId, remotePaths) => {
        if (remotePaths.length === 0) return;
        const dest = await resolveDestination(get, set, false);
        if (!dest) return;
        await api.sftpTransferStart(
          sessionId,
          "download",
          remotePaths,
          dest,
          get().concurrency,
        );
        set({ open: true });
      },

      downloadArchive: async (sessionId, remoteDir, format) => {
        const dest = await resolveDestination(get, set, false);
        if (!dest) return;
        await api.sftpArchiveStart(sessionId, remoteDir, dest, format);
        set({ open: true });
      },

      upload: async (sessionId, remoteDir, localPaths) => {
        const files = localPaths ?? (await api.pickFiles("Choose files to upload"));
        if (files.length === 0) return;
        await api.sftpTransferStart(
          sessionId,
          "upload",
          files,
          remoteDir,
          get().concurrency,
        );
        set({ open: true });
      },

      cancel: async (id) => {
        await api.sftpTransferCancel(id);
      },

      clearFinished: async () => {
        await api.sftpTransferClearFinished();
        set((s) => ({
          jobs: Object.fromEntries(
            Object.entries(s.jobs).filter(
              ([, j]) => j.state === "scanning" || j.state === "running",
            ),
          ),
        }));
      },
    }),
    {
      name: "xconsole-transfers",
      version: 1,
      // Jobs live in the backend; only the user's preferences are worth persisting.
      partialize: (s) => ({
        concurrency: s.concurrency,
        lastDestination: s.lastDestination,
      }),
    },
  ),
);
