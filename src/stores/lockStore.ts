import { create } from "zustand";
import { api } from "../lib/tauri";

// App-lock gate state. On launch the app asks the backend whether a lock is configured and
// whether it's already unlocked (silently, from the OS keychain). If it's locked, the whole
// app is held behind the unlock screen until the master password decrypts the data.

type Status = "loading" | "locked" | "unlocked";

interface LockState {
  status: Status;
  /** This device has the key remembered (silent unlock at launch). */
  remembered: boolean;
  error: string | null;
  busy: boolean;
  check: () => Promise<void>;
  unlock: (password: string, remember: boolean) => Promise<boolean>;
}

export const useLockStore = create<LockState>((set) => ({
  status: "loading",
  remembered: false,
  error: null,
  busy: false,

  check: async () => {
    try {
      const s = await api.lockStatus();
      // Locked only when a lock is configured AND not already unlocked (no remembered key).
      set({
        status: s.enabled && !s.unlocked ? "locked" : "unlocked",
        remembered: s.remembered,
      });
    } catch {
      // Never brick the app over a status read — fail open to the normal UI.
      set({ status: "unlocked" });
    }
  },

  unlock: async (password, remember) => {
    set({ busy: true, error: null });
    try {
      await api.unlockWithPassword(password, remember);
      set({ status: "unlocked", error: null, busy: false });
      return true;
    } catch (e) {
      set({ error: String(e), busy: false });
      return false;
    }
  },
}));
