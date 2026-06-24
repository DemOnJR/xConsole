import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

// Drives the auto-update flow: check GitHub for a newer signed release, and on the
// user's accept, download + install + relaunch. The user's data is untouched — an
// update only replaces the app binary (data lives in the OS app-data dir / keychain).

type Status =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "uptodate"
  | "error";

interface UpdateState {
  status: Status;
  version: string | null;
  notes: string | null;
  /** 0..1 download progress. */
  progress: number;
  error: string | null;
  dismissed: boolean;
  /** The pending update handle from the plugin. */
  pending: Update | null;

  /** Check for an update. `manual` shows "up to date"/errors; silent checks stay quiet. */
  check: (manual: boolean) => Promise<void>;
  /** Download + install the pending update, then relaunch. */
  install: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: "idle",
  version: null,
  notes: null,
  progress: 0,
  error: null,
  dismissed: false,
  pending: null,

  check: async (manual) => {
    const s = get().status;
    if (s === "checking" || s === "downloading" || s === "installing") return;
    set({ status: "checking", error: null, dismissed: false });
    try {
      const upd = await check();
      if (upd) {
        set({
          status: "available",
          version: upd.version,
          notes: upd.body ?? "",
          pending: upd,
        });
      } else {
        // No newer release. Only surface this for an explicit manual check.
        set({ status: manual ? "uptodate" : "idle" });
      }
    } catch (e) {
      // Silent checks (offline, no release published yet) must not nag the user.
      set(manual ? { status: "error", error: String(e) } : { status: "idle" });
    }
  },

  install: async () => {
    const upd = get().pending;
    if (!upd) return;
    set({ status: "downloading", progress: 0, error: null });
    try {
      let total = 0;
      let got = 0;
      await upd.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? 0;
        } else if (ev.event === "Progress") {
          got += ev.data.chunkLength;
          set({ progress: total ? Math.min(1, got / total) : 0 });
        } else if (ev.event === "Finished") {
          set({ progress: 1, status: "installing" });
        }
      });
      // Installed — restart into the new version (data is preserved).
      await relaunch();
    } catch (e) {
      set({ status: "error", error: String(e) });
    }
  },

  dismiss: () => set({ dismissed: true, status: "idle" }),
}));
