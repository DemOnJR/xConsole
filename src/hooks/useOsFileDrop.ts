import { useEffect } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

type DropHandler = (target: string, paths: string[]) => void;

/** Registered by whoever cares about files being dropped on a `data-drop` target. */
const handlers = new Set<DropHandler>();
/** The target currently under a dragged file, for highlighting. */
let hoverTarget: string | null = null;
const hoverListeners = new Set<(t: string | null) => void>();

function setHover(t: string | null) {
  if (t === hoverTarget) return;
  hoverTarget = t;
  hoverListeners.forEach((f) => f(t));
}

export function onOsFilesDropped(fn: DropHandler): () => void {
  handlers.add(fn);
  return () => handlers.delete(fn);
}

export function onOsDropHover(fn: (t: string | null) => void): () => void {
  hoverListeners.add(fn);
  return () => hoverListeners.delete(fn);
}

/**
 * Files dragged in from Explorer.
 *
 * Tauri reports these as a window-level event with a cursor position rather than a DOM
 * event on an element, so the drop target is found by hit-testing that point. The position
 * arrives in **physical** pixels while `elementFromPoint` wants CSS pixels, which is a
 * silent one-monitor-works-one-doesn't bug on any display that is not at 100% scaling —
 * hence the explicit `devicePixelRatio` division.
 *
 * Mounted once, at app level.
 */
export function useOsFileDrop() {
  useEffect(() => {
    let un: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        const dpr = window.devicePixelRatio || 1;
        const at = (pos: { x: number; y: number }) => ({
          x: pos.x / dpr,
          y: pos.y / dpr,
        });

        if (p.type === "over") {
          const { x, y } = at(p.position);
          const el = document.elementFromPoint(x, y);
          setHover(el?.closest<HTMLElement>("[data-drop]")?.dataset.drop ?? null);
          return;
        }
        if (p.type === "leave") {
          setHover(null);
          return;
        }
        if (p.type === "drop") {
          const { x, y } = at(p.position);
          const el = document.elementFromPoint(x, y);
          const target = el?.closest<HTMLElement>("[data-drop]")?.dataset.drop ?? null;
          setHover(null);
          if (!target || !p.paths?.length) return;
          handlers.forEach((fn) => fn(target, p.paths));
        }
      })
      .then((f) => (un = f));
    return () => un?.();
  }, []);
}
