import { useEffect, useRef } from "react";
import { useReactFlow } from "@xyflow/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/tauri";
import { workspacePersistKey } from "../lib/workspacePersist";
import { useCanvasStore } from "../stores/canvasStore";
import { useWorkspaceStore } from "../stores/workspaceStore";

/** Backend setting holding the workspace that was open when the app last closed. */
const ACTIVE_KEY = "ui.active_workspace";
/** Quiet period after a change before writing. Dragging a node fires continuously. */
const DEBOUNCE_MS = 800;
/** Hard cap on the save-before-close. A wedged backend must never trap the window. */
const SAVE_ON_CLOSE_TIMEOUT_MS = 2500;

/**
 * Keep the open workspace saved, and put it back exactly as it was on the next launch.
 *
 * Layout was already persisted per workspace — what was missing was that nothing recorded
 * *which* workspace was open, nothing wrote the layout unless the user pressed Save, and
 * the canvas ran `fitView` on mount, which overruled the saved viewport anyway. The result
 * was that reopening the app rearranged everything.
 *
 * Saving goes through the same `save` the Save button uses, so there is one code path and
 * no second format to keep in step.
 *
 * The active-workspace id lives in the (encrypted) settings table rather than
 * localStorage: it names a server layout, and it must not be readable while the app is
 * locked.
 */
export function useWorkspaceAutosave() {
  const { getViewport, setViewport } = useReactFlow();
  const restoredRef = useRef(false);
  const timer = useRef<number | null>(null);
  const lastSavedRef = useRef<string | null>(null);
  const savingRef = useRef(false);

  // ----- Restore, once, on launch -----
  useEffect(() => {
    if (restoredRef.current) return;
    restoredRef.current = true;
    void (async () => {
      const id = await api.getSetting(ACTIVE_KEY).catch(() => null);
      if (!id) return;
      await useWorkspaceStore.getState().load();
      const res = await useWorkspaceStore.getState().restore(id);
      if (!res) return;
      const canvas = useCanvasStore.getState();
      canvas.setNodes(res.nodes);
      canvas.setEdges(res.edges);
      // Order matters: the saved arrangement has to be installed before the mode, or
      // switching to "tile" re-tiles from scratch and discards the shape that was saved.
      canvas.setTileLayout(res.tiles);
      canvas.setLayout(res.layout);
      // Tiles are laid out in flow coordinates anchored at the origin, so they only line
      // up at 1:1; everything else gets the viewport it was left at, verbatim.
      if (res.layout !== "tile") setViewport(res.viewport);
      lastSavedRef.current = workspacePersistKey(
        useCanvasStore.getState(),
        res.layout === "tile" ? getViewport() : res.viewport,
      );
    })();
  }, [setViewport]);

  // ----- Autosave on change -----
  useEffect(() => {
    const persistKey = () =>
      workspacePersistKey(useCanvasStore.getState(), getViewport());

    const flush = async () => {
      const { activeId, workspaces, save } = useWorkspaceStore.getState();
      if (!activeId) return;
      const ws = workspaces.find((w) => w.id === activeId);
      if (!ws) return;
      const key = persistKey();
      if (key === lastSavedRef.current) return;
      savingRef.current = true;
      try {
        await save(
          ws.name,
          getViewport(),
          ws.id,
          ws.color ?? undefined,
          ws.icon ?? undefined,
          ws.color_mode ?? undefined,
        );
        lastSavedRef.current = persistKey();
      } catch {
        // A failed save must not wedge the next one.
      } finally {
        savingRef.current = false;
      }
    };

    const schedule = () => {
      if (!restoredRef.current || savingRef.current) return;
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void flush(), DEBOUNCE_MS);
    };

    // Layout only. Focus, WebGL LRU, pane-size ticks, and select/measure noise
    // used to resave the workspace every 800 ms, which rewrote the encrypted DB
    // at ~10 MB/s. Save() used to setNodes back into the store and retrigger this.
    const unsubCanvas = useCanvasStore.subscribe(() => {
      if (savingRef.current) return;
      if (persistKey() === lastSavedRef.current) return;
      schedule();
    });
    // Switching workspaces changes which one should be remembered.
    const unsubWs = useWorkspaceStore.subscribe((s, prev) => {
      if (s.activeId === prev.activeId) return;
      void api.setSetting(ACTIVE_KEY, s.activeId ?? "").catch(() => {});
    });

    // Closing the window is the one moment a debounce would lose work, so save first and
    // only then let it close.
    //
    // Every branch here exists because getting this wrong makes the app **impossible to
    // close except from Task Manager**: once `preventDefault` has run, anything that
    // throws or hangs before the window is actually closed traps the user. So:
    //
    //   * the save is raced against a timeout — a wedged backend must not hold the window
    //     open, and losing the last few seconds of layout is the lesser evil;
    //   * every await is guarded, and the close happens in `finally`;
    //   * the close is a plain `close()`, which re-enters this handler and is waved
    //     through by the guard. `destroy()` also works but tears the webview down
    //     without running the normal shutdown, which showed up as the process exiting
    //     with a non-zero code on alternate runs. `destroy` stays as the last resort
    //     for the case where `close` itself fails.
    let unlistenClose: (() => void) | undefined;
    let closeHandled = false;
    const win = getCurrentWindow();
    void win
      .onCloseRequested(async (event) => {
        // Second request — we have already saved. Let it close.
        if (closeHandled) return;
        closeHandled = true;
        event.preventDefault();
        if (timer.current != null) clearTimeout(timer.current);
        try {
          await Promise.race([
            (async () => {
              await flush();
              const { activeId } = useWorkspaceStore.getState();
              await api.setSetting(ACTIVE_KEY, activeId ?? "").catch(() => {});
            })(),
            new Promise((r) => setTimeout(r, SAVE_ON_CLOSE_TIMEOUT_MS)),
          ]);
        } catch {
          // Saving failed; closing must still happen.
        } finally {
          try {
            await win.close();
          } catch {
            await win.destroy().catch(() => {});
          }
        }
      })
      .then((f) => (unlistenClose = f));

    return () => {
      unsubCanvas();
      unsubWs();
      unlistenClose?.();
      if (timer.current != null) clearTimeout(timer.current);
    };
  }, [getViewport]);
}
