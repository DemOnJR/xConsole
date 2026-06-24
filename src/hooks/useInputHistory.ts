import { useRef } from "react";

// Time-coalesced undo/redo for a controlled text input. The component records
// every change; rapid keystrokes (<500ms apart) collapse into one undo step so
// Ctrl+Z reverts edits in sensible chunks rather than one character at a time.

const COALESCE_MS = 500;
const MAX_HISTORY = 200;

export function useInputHistory(setValue: (v: string) => void) {
  const hist = useRef<string[]>([""]);
  const idx = useRef(0);
  const lastAt = useRef(0);

  const record = (next: string) => {
    const h = hist.current;
    // Drop any redo tail when a new edit diverges.
    if (idx.current < h.length - 1) h.splice(idx.current + 1);
    const now = Date.now();
    if (now - lastAt.current < COALESCE_MS && idx.current === h.length - 1) {
      h[idx.current] = next; // coalesce into the current step
    } else {
      h.push(next);
      idx.current = h.length - 1;
    }
    lastAt.current = now;
    if (h.length > MAX_HISTORY) {
      h.shift();
      idx.current = Math.max(0, idx.current - 1);
    }
  };

  const undo = (): boolean => {
    if (idx.current > 0) {
      idx.current -= 1;
      setValue(hist.current[idx.current]);
      lastAt.current = 0; // next typed change starts a fresh step
      return true;
    }
    return false;
  };

  const redo = (): boolean => {
    if (idx.current < hist.current.length - 1) {
      idx.current += 1;
      setValue(hist.current[idx.current]);
      lastAt.current = 0;
      return true;
    }
    return false;
  };

  /** Reset the history to a single baseline (e.g. after sending). */
  const reset = (v = "") => {
    hist.current = [v];
    idx.current = 0;
    lastAt.current = 0;
  };

  return { record, undo, redo, reset };
}
