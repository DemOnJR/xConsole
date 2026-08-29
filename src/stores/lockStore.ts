import { create } from "zustand";
import { api } from "../lib/tauri";
import { useSessionStore } from "./sessionStore";

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
  /** Drop back to the unlock screen. Called when the backend locks (idle timeout, or
   *  the user pressing Lock) — the backend has already closed the shells and thrown the
   *  key away, so this only catches the UI up. */
  setLocked: () => void;
  /** Lock on demand. */
  lockNow: () => Promise<void>;
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

  setLocked: () => {
    useSessionStore.getState().clear();
    set({ status: "locked", error: null, busy: false });
  },

  lockNow: async () => {
    useSessionStore.getState().clear();
    try {
      await api.lockNow();
    } catch (e) {
      // Report it, but still show the lock screen: `lock_now` throws only when the save
      // failed *after* the key was already dropped, so the app really is locked.
      set({ error: String(e) });
    }
    set({ status: "locked", busy: false });
  },
}));
