import { create } from "zustand";
import { api, type ChannelInfo, type UpdateInfo } from "../lib/tauri";

// Drives the in-app update flow for the clone+compile distribution: ask the backend
// to compare the local checkout against the selected channel (main / dev) on GitHub,
// and on the user's accept back up their data + re-run the installer for that branch.
// User data is never touched by the rebuild — and is snapshotted to a backup first.

type Status = "idle" | "checking" | "available" | "updating" | "uptodate" | "error";

export type UpdateChannel = "main" | "dev";

interface UpdateState {
  status: Status;
  /** Short SHA of the latest commit on the active channel. */
  version: string | null;
  /** Latest commit message ("what's new"). */
  notes: string | null;
  /** Short SHA the app was built from. */
  current: string | null;
  /** Active update channel (main = stable, dev = development). */
  channel: UpdateChannel;
  /** Local checkout branch, when known. */
  localBranch: string | null;
  /** Extra note (e.g. channel mismatch). */
  note: string | null;
  /** Whether the in-place updater (installer) is present. */
  canSelfUpdate: boolean;
  error: string | null;
  dismissed: boolean;

  /** Load channel + local SHA without a full GitHub check. */
  loadChannel: () => Promise<void>;
  /** Switch channel (main/dev). Does not rebuild until the user updates. */
  setChannel: (channel: UpdateChannel) => Promise<void>;
  /** Check for an update. `manual` shows "up to date"/errors; silent checks stay quiet. */
  check: (manual: boolean) => Promise<void>;
  /** Back up data + launch the installer rebuild for the active channel. */
  install: () => Promise<void>;
  dismiss: () => void;
}

function asChannel(s: string | null | undefined): UpdateChannel {
  return s === "dev" ? "dev" : "main";
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: "idle",
  version: null,
  notes: null,
  current: null,
  channel: "main",
  localBranch: null,
  note: null,
  canSelfUpdate: false,
  error: null,
  dismissed: false,

  loadChannel: async () => {
    try {
      const info: ChannelInfo = await api.getUpdateChannel();
      set({
        channel: asChannel(info.channel),
        localBranch: info.local_branch,
        current: info.current,
        canSelfUpdate: info.can_self_update,
      });
    } catch {
      /* non-fatal — defaults stay */
    }
  },

  setChannel: async (channel) => {
    try {
      const info = await api.setUpdateChannel(channel);
      set({
        channel: asChannel(info.channel),
        localBranch: info.local_branch,
        current: info.current,
        canSelfUpdate: info.can_self_update,
        status: "idle",
        dismissed: false,
      });
      // Immediately see if the new channel has a different build waiting.
      await get().check(true);
    } catch (e) {
      set({ status: "error", error: String(e) });
    }
  },

  check: async (manual) => {
    const s = get().status;
    if (s === "checking" || s === "updating") return;
    set({ status: "checking", error: null, dismissed: false });
    try {
      const info: UpdateInfo = await api.checkForUpdate();
      set({
        channel: asChannel(info.channel),
        localBranch: info.local_branch,
        current: info.current,
        canSelfUpdate: info.can_self_update,
        note: info.note,
      });
      if (info.available && info.can_self_update) {
        set({
          status: "available",
          version: info.latest,
          notes: info.message || "A newer version is available.",
        });
      } else {
        set({ status: manual ? "uptodate" : "idle" });
      }
    } catch (e) {
      set(manual ? { status: "error", error: String(e) } : { status: "idle" });
    }
  },

  install: async () => {
    if (get().status === "updating") return;
    set({ status: "updating", error: null });
    try {
      await api.startAppUpdate();
    } catch (e) {
      set({ status: "error", error: String(e) });
    }
  },

  dismiss: () => set({ dismissed: true, status: "idle" }),
}));
