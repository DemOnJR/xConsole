import { useEffect, useRef } from "react";
import { useReactFlow } from "@xyflow/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/tauri";
import { useCanvasStore } from "../stores/canvasStore";
import { useWorkspaceStore } from "../stores/workspaceStore";

/** Backend setting holding the workspace that was open when the app last closed. */
const ACTIVE_KEY = "ui.active_workspace";
/** Quiet period after a change before writing. Dragging a node fires continuously. */
const DEBOUNCE_MS = 800;

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
    })();
  }, [setViewport]);

  // ----- Autosave on change -----
  useEffect(() => {
    const flush = async () => {
      const { activeId, workspaces, save } = useWorkspaceStore.getState();
      if (!activeId) return;
      const ws = workspaces.find((w) => w.id === activeId);
      if (!ws) return;
      await save(
        ws.name,
        getViewport(),
        ws.id,
        ws.color ?? undefined,
        ws.icon ?? undefined,
        ws.color_mode ?? undefined,
      ).catch(() => {});
    };

    const schedule = () => {
      if (!restoredRef.current) return;
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void flush(), DEBOUNCE_MS);
    };

    // Any change to what is on the canvas or how it is arranged.
    const unsubCanvas = useCanvasStore.subscribe(schedule);
    // Switching workspaces changes which one should be remembered.
    const unsubWs = useWorkspaceStore.subscribe((s, prev) => {
      if (s.activeId === prev.activeId) return;
      void api.setSetting(ACTIVE_KEY, s.activeId ?? "").catch(() => {});
    });

    // Closing the window is the one moment a debounce would lose work, so save first and
    // only then let it close. `destroy` rather than `close` avoids re-entering this
    // handler with the same request.
    let unlistenClose: (() => void) | undefined;
    const win = getCurrentWindow();
    void win
      .onCloseRequested(async (event) => {
        event.preventDefault();
        if (timer.current != null) clearTimeout(timer.current);
        await flush();
        const { activeId } = useWorkspaceStore.getState();
        await api.setSetting(ACTIVE_KEY, activeId ?? "").catch(() => {});
        await win.destroy();
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
