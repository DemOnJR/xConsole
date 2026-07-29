import { useEffect } from "react";
import { useCanvasStore } from "../stores/canvasStore";

/** How much one keypress changes a tile's width/height share. */
const GROW_STEP = 0.25;

/**
 * Should this keystroke be left alone because the user is typing into a real field?
 *
 * xterm.js runs the terminal through a hidden `<textarea>`, so a blanket "ignore
 * textareas" rule would kill every shortcut while a terminal has focus — which is
 * exactly when they are most useful. Terminals are therefore allowed through, and
 * only genuine form fields opt out.
 */
function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  if (el.closest(".xterm")) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}

/**
 * Layout keys for tile mode. Alt-based throughout: React Flow already claims Shift
 * (selection box), Control/Meta (multi-select + zoom), Space (pan) and the bare arrow
 * keys (nudge a node by 1px), and the app claims Ctrl+Tab.
 *
 *   Alt + ← / →          move the tile within its row
 *   Alt + ↑ / ↓          move the tile to the row above / below (past the edge = its own row)
 *   Alt + Shift + ← / →  make the tile narrower / wider
 *   Alt + Shift + ↑ / ↓  make its row shorter / taller
 *   Alt + F              give the tile a full-width row of its own — press again to merge back
 *   Alt + R              reset to the balanced default arrangement
 *
 * The listener is registered in the **capture** phase: xterm attaches its own keydown
 * handler to the terminal element, and a bubble-phase listener would only see the key
 * after it had already been forwarded to the remote shell.
 */
export function useTileShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      if (isTypingTarget(e.target)) return;

      const store = useCanvasStore.getState();
      if (store.layoutMode !== "tile" || store.nodes.length === 0) return;

      // Act on the focused tile, falling back to the first one so the keys still do
      // something sensible before the user has clicked anything.
      const id =
        store.focusedId && store.nodes.some((n) => n.id === store.focusedId)
          ? store.focusedId
          : store.nodes[0].id;

      const key = e.key;
      let handled = true;

      switch (key) {
        case "ArrowLeft":
          if (e.shiftKey) store.growTile(id, -GROW_STEP, "horizontal");
          else store.moveTile(id, -1, "horizontal");
          break;
        case "ArrowRight":
          if (e.shiftKey) store.growTile(id, GROW_STEP, "horizontal");
          else store.moveTile(id, 1, "horizontal");
          break;
        case "ArrowUp":
          if (e.shiftKey) store.growTile(id, -GROW_STEP, "vertical");
          else store.moveTile(id, -1, "vertical");
          break;
        case "ArrowDown":
          if (e.shiftKey) store.growTile(id, GROW_STEP, "vertical");
          else store.moveTile(id, 1, "vertical");
          break;
        case "f":
        case "F":
          store.toggleTileFullWidth(id);
          break;
        case "r":
        case "R":
          store.resetTileLayout();
          break;
        default:
          handled = false;
      }

      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    };

    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
}
