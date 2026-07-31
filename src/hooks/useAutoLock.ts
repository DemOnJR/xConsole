import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/tauri";
import { useLockStore } from "../stores/lockStore";

/**
 * Two halves of the idle auto-lock, both of which have to exist for it to be safe.
 *
 * 1. **Report activity.** The timeout is enforced in the backend (a JS timer dies with a
 *    hung webview), but only the webview can see real input, so it has to say when there
 *    was some. Without this the app would lock on a fixed schedule *while being used*,
 *    which trains people to turn the lock off — the worst possible outcome.
 * 2. **React to being locked.** The backend can lock on its own, so the UI cannot assume
 *    it is unlocked just because it once was. `app://locked` flips the gate back.
 *
 * Activity is throttled to one IPC call per `REPORT_EVERY_MS`. The events are captured at
 * the document, because xterm swallows key events before they bubble.
 */
const REPORT_EVERY_MS = 20_000;

export function useAutoLock() {
  const setLocked = useLockStore((s) => s.setLocked);

  useEffect(() => {
    let last = 0;
    const report = () => {
      const now = Date.now();
      if (now - last < REPORT_EVERY_MS) return;
      last = now;
      void api.noteActivity().catch((e) => {
        // Never surfaced as an error dialog, but never silent either: if this stops
        // working the backend cannot measure idleness, and it deliberately stops
        // auto-locking rather than firing blindly. That needs to be diagnosable.
        console.warn("xconsole: idle heartbeat failed", e);
      });
    };

    // "Activity" is deliberately input only. Timers, streaming terminal output and
    // background jobs are not a person at the keyboard, and counting them would keep an
    // unattended machine unlocked for as long as something was running — exactly the
    // case the timeout is for.
    const events = ["keydown", "mousedown", "wheel", "touchstart"] as const;
    for (const e of events) {
      document.addEventListener(e, report, { capture: true, passive: true });
    }
    // Count the mount itself, so a fresh unlock starts with a full timeout.
    void api.noteActivity().catch(() => {});

    const un = listen<number>("app://locked", () => setLocked());

    return () => {
      for (const e of events) {
        document.removeEventListener(e, report, { capture: true });
      }
      void un.then((f) => f());
    };
  }, [setLocked]);
}
