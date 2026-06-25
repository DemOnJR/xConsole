import { create } from "zustand";
import { api, type UpdateInfo } from "../lib/tauri";

// Drives the in-app update flow for the clone+compile distribution: ask the backend
// to compare the local checkout against origin/main on GitHub, and on the user's accept
// back up their data + re-run the installer (git pull + rebuild + swap the exe). The
// user's data (DB, agent memory, chats, workspaces, settings, keychain) is never touched
// by the rebuild — and is snapshotted to a backup first.

type Status = "idle" | "checking" | "available" | "updating" | "uptodate" | "error";

interface UpdateState {
  status: Status;
  /** Short SHA of the latest commit on GitHub. */
  version: string | null;
  /** Latest commit message ("what's new"). */
  notes: string | null;
  /** Short SHA the app was built from. */
  current: string | null;
  /** Whether the in-place updater (installer) is present. */
  canSelfUpdate: boolean;
  error: string | null;
  dismissed: boolean;

  /** Check for an update. `manual` shows "up to date"/errors; silent checks stay quiet. */
  check: (manual: boolean) => Promise<void>;
  /** Back up data + launch the installer rebuild. */
  install: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: "idle",
  version: null,
  notes: null,
  current: null,
  canSelfUpdate: false,
  error: null,
  dismissed: false,

  check: async (manual) => {
    const s = get().status;
    if (s === "checking" || s === "updating") return;
    set({ status: "checking", error: null, dismissed: false });
    try {
      const info: UpdateInfo = await api.checkForUpdate();
      if (info.available && info.can_self_update) {
        set({
          status: "available",
          version: info.latest,
          current: info.current,
          notes: info.message || "A newer version is available.",
          canSelfUpdate: info.can_self_update,
        });
      } else {
        // Up to date, or can't self-update (installed some other way) — only surface
        // for an explicit manual check.
        set({ status: manual ? "uptodate" : "idle", canSelfUpdate: info.can_self_update });
      }
    } catch (e) {
      set(manual ? { status: "error", error: String(e) } : { status: "idle" });
    }
  },

  install: async () => {
    if (get().status === "updating") return;
    set({ status: "updating", error: null });
    try {
      // Backs up data, then launches the installer's rebuild in its own window. That
      // installer stops this app before swapping the exe, so the app will close on its
      // own partway through; the user reopens the (updated) app when the installer finishes.
      await api.startAppUpdate();
    } catch (e) {
      set({ status: "error", error: String(e) });
    }
  },

  dismiss: () => set({ dismissed: true, status: "idle" }),
}));
