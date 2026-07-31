import { create } from "zustand";

/**
 * Internal drag-and-drop, built on pointer events instead of HTML5 DnD.
 *
 * HTML5 drag events do not fire once Tauri's `dragDropEnabled` is on, because the webview
 * hands native drags to the OS layer so the app can receive dropped *files*. We need both:
 * files dropped in from Explorer, and servers/remote files dragged around inside the app.
 * Pointer events are unaffected by that setting, so internal drags use them and the two
 * mechanisms stop competing.
 *
 * Drop targets are plain DOM: any element carrying `data-drop="<id>"`. On release the
 * element under the cursor is hit-tested with `elementFromPoint`, which means targets need
 * no registration and work inside React Flow's transformed canvas.
 */

export type DragKind = "vps" | "remote-file";

export interface DragPayload {
  kind: DragKind;
  /** VPS id, or the SFTP session's vps id for a remote file. */
  vpsId: string;
  /** Remote absolute path (remote-file only). */
  path?: string;
  /** Display label for the ghost. */
  label: string;
  /** Remote-file only: directory rather than file. */
  isDir?: boolean;
}

interface DragState {
  drag: DragPayload | null;
  x: number;
  y: number;
  /** data-drop id currently under the cursor, for highlighting. */
  over: string | null;
  begin: (p: DragPayload, x: number, y: number) => void;
  move: (x: number, y: number, over: string | null) => void;
  end: () => void;
}

export const useDragStore = create<DragState>((set) => ({
  drag: null,
  x: 0,
  y: 0,
  over: null,
  begin: (drag, x, y) => set({ drag, x, y, over: null }),
  move: (x, y, over) => set({ x, y, over }),
  end: () => set({ drag: null, over: null }),
}));

/** The `data-drop` id under a viewport point, if any. */
export function dropTargetAt(x: number, y: number): string | null {
  const el = document.elementFromPoint(x, y);
  const target = el?.closest<HTMLElement>("[data-drop]");
  return target?.dataset.drop ?? null;
}

type DropFn = (payload: DragPayload, x: number, y: number, target: string) => void;
const dropHandlers = new Map<string, Set<DropFn>>();

/**
 * Handle internal drops on a target.
 *
 * `key` matches a `data-drop` id exactly, or as a prefix when the id is `key:something` —
 * so a list can register once for all its rows while a single node registers for itself.
 */
export function onInternalDrop(key: string, fn: DropFn): () => void {
  let set = dropHandlers.get(key);
  if (!set) {
    set = new Set();
    dropHandlers.set(key, set);
  }
  set.add(fn);
  return () => {
    set!.delete(fn);
    if (set!.size === 0) dropHandlers.delete(key);
  };
}

function dispatchDrop(target: string, payload: DragPayload, x: number, y: number) {
  const keys = [target];
  const colon = target.indexOf(":");
  if (colon > 0) keys.push(target.slice(0, colon));
  for (const k of keys) {
    dropHandlers.get(k)?.forEach((fn) => fn(payload, x, y, target));
  }
}

/**
 * Start an internal drag from a pointerdown.
 *
 * Nothing happens until the pointer has actually moved a few pixels, so a plain click on a
 * file row still selects it rather than starting a drag nobody asked for.
 */
export function startInternalDrag(
  e: React.PointerEvent,
  payload: DragPayload,
  onDrop?: (target: string, payload: DragPayload) => void,
) {
  const startX = e.clientX;
  const startY = e.clientY;
  let armed = false;
  const THRESHOLD = 4;

  const onMove = (ev: PointerEvent) => {
    if (!armed) {
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < THRESHOLD) return;
      armed = true;
      useDragStore.getState().begin(payload, ev.clientX, ev.clientY);
    }
    useDragStore
      .getState()
      .move(ev.clientX, ev.clientY, dropTargetAt(ev.clientX, ev.clientY));
  };

  const onUp = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    if (!armed) return;
    const target = dropTargetAt(ev.clientX, ev.clientY);
    useDragStore.getState().end();
    if (!target) return;
    onDrop?.(target, payload);
    dispatchDrop(target, payload, ev.clientX, ev.clientY);
  };

  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
}
