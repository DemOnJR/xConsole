import { useEffect, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import {
  clipboardImagePng,
  copyToClipboard,
  pasteFromClipboard,
} from "../lib/terminalClipboard";

/** Two Ctrl+C presses closer together than this mean "interrupt", not "copy". */
const DOUBLE_TAP_MS = 500;
/** How long the "press again to interrupt" nudge stays on screen. */
const HINT_MS = 1400;

export interface TerminalClipboardOpts {
  term: React.RefObject<Terminal | null>;
  host: React.RefObject<HTMLElement | null>;
  /** Write raw bytes to the shell (already-encoded string). */
  send: (data: string) => void;
  /** Handle pasted image data — uploaded and turned into a remote path. */
  onImage?: (png: Uint8Array) => void;
}

/**
 * Terminal copy/paste, wired the way a terminal user expects rather than the way a
 * browser does.
 *
 * The awkward part is Ctrl+C, which has meant "interrupt" for fifty years and "copy"
 * for thirty. Rather than pick a winner, a **single** press copies the selection and a
 * **double** press interrupts. The nudge matters: a user whose Ctrl+C did not stop a
 * runaway process needs to be told why immediately, or they will assume the terminal has
 * hung. Ctrl+Shift+C / Ctrl+Shift+V keep working as the unambiguous forms.
 *
 * Returns a transient hint string for the node to render.
 */
export function useTerminalClipboard({
  term,
  host,
  send,
  onImage,
}: TerminalClipboardOpts) {
  const [hint, setHint] = useState<string | null>(null);
  const hintTimer = useRef<number | null>(null);
  const lastCtrlC = useRef(0);
  // Read in listeners that are attached once; keeps them off the effect's dep list.
  const sendRef = useRef(send);
  sendRef.current = send;
  const onImageRef = useRef(onImage);
  onImageRef.current = onImage;

  const flash = (msg: string) => {
    setHint(msg);
    if (hintTimer.current != null) clearTimeout(hintTimer.current);
    hintTimer.current = window.setTimeout(() => setHint(null), HINT_MS);
  };

  useEffect(() => {
    const t = term.current;
    const el = host.current;
    if (!t || !el) return;

    const copySelection = (): boolean => {
      const sel = t.getSelection();
      if (!sel) return false;
      void copyToClipboard(sel);
      t.clearSelection();
      flash("Copied");
      return true;
    };

    const doPaste = async () => {
      // Image first: a clipboard holding a screenshot usually also holds a filename or
      // some stray text, and the image is what the user meant.
      if (onImageRef.current) {
        const png = await clipboardImagePng();
        if (png) {
          onImageRef.current(png);
          return;
        }
      }
      const text = await pasteFromClipboard();
      if (text) sendRef.current(text);
    };

    t.attachCustomKeyEventHandler((e: KeyboardEvent) => {
      if (e.type !== "keydown") return true;
      const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
      if (!ctrl) return true;

      const key = e.key.toLowerCase();

      // Returning false only tells xterm to keep its hands off the key — the browser
      // still runs its own default, and for paste that means the hidden textarea gets a
      // native `paste` event which xterm dutifully writes to the shell. Together with our
      // own clipboard read that pasted everything twice. preventDefault is what actually
      // stops the second copy.
      const claim = () => {
        e.preventDefault();
        return false;
      };

      // Unambiguous forms: always copy / always paste.
      if (e.shiftKey && key === "c") {
        copySelection();
        return claim();
      }
      if (e.shiftKey && key === "v") {
        void doPaste();
        return claim();
      }
      if (e.shiftKey) return true;

      if (key === "c") {
        const now = Date.now();
        if (now - lastCtrlC.current < DOUBLE_TAP_MS) {
          // Second press: this is the interrupt.
          lastCtrlC.current = 0;
          t.clearSelection();
          sendRef.current("\x03");
          return claim();
        }
        lastCtrlC.current = now;
        if (copySelection()) return claim();
        // Nothing to copy, so the user almost certainly meant to interrupt. Do not
        // send it yet — that would make the double-tap rule a lie — but say so.
        flash("Press Ctrl+C again to interrupt");
        return claim();
      }

      if (key === "v") {
        void doPaste();
        return claim();
      }
      return true;
    });

    // Ctrl+left-drag: copy as soon as the selection is released, no keystroke needed.
    const onMouseUp = (e: MouseEvent) => {
      if (e.button !== 0 || !e.ctrlKey) return;
      copySelection();
    };
    // Ctrl+right-click: paste. Right-click alone is left free for the context menu.
    const onContextMenu = (e: MouseEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      void doPaste();
    };

    el.addEventListener("mouseup", onMouseUp);
    el.addEventListener("contextmenu", onContextMenu);
    return () => {
      el.removeEventListener("mouseup", onMouseUp);
      el.removeEventListener("contextmenu", onContextMenu);
      if (hintTimer.current != null) clearTimeout(hintTimer.current);
    };
    // Attached once, after the terminal exists; the refs above keep it current.
  }, [term, host]);

  return hint;
}
