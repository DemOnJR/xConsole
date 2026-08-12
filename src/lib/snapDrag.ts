/**
 * Snap-drag state: who is being dragged, where, and the zone under the cursor.
 *
 * The node headers start a snap drag with `beginSnapDrag`; the overlay reads this
 * state to draw the zones and highlight. `endSnapDrag` applies the snapped layout
 * via the canvas store (switching to tile mode) when a zone is hit.
 */
import { create } from "zustand";
import { useCanvasStore } from "../stores/canvasStore";
import { snapZones, zoneAt, snapLayout, type SnapZone } from "./snapLayout";

interface SnapDragState {
  /** The node being dragged, or null when idle. */
  nodeId: string | null;
  /** Cursor position in pane coordinates (0..1). */
  px: number;
  py: number;
  /** Total node count when the drag started. */
  count: number;
  /** The layout that was active when the drag started (restored on a miss). */
  prevLayout: ReturnType<typeof useCanvasStore.getState>["tileLayout"];
  /** The layout mode at drag start, so a miss restores it too. */
  prevMode: "freeform" | "tile";
  /**
   * Whether the preview is "armed" — the cursor has entered a zone's trigger band.
   * Windows only shows the layout overlay once you get close to a snap position.
   */
  armed: boolean;
  /** The zone the cursor is currently in (highlighted). */
  zone: SnapZone | null;
  begin: (nodeId: string) => void;
  move: (px: number, py: number) => void;
  end: () => void;
}

export const useSnapDragStore = create<SnapDragState>((set) => ({
  nodeId: null,
  px: 0,
  py: 0,
  count: 0,
  prevLayout: null,
  prevMode: "freeform",
  armed: false,
  zone: null,
  begin: (nodeId) => {
    const canvas = useCanvasStore.getState();
    set({
      nodeId,
      px: 0,
      py: 0,
      count: canvas.nodes.length,
      prevLayout: canvas.tileLayout,
      prevMode: canvas.layoutMode,
      armed: false,
      zone: null,
    });
  },
  move: (px, py) => {
    const { count } = useSnapDragStore.getState();
    const zones = snapZones(count);
    const zone = zoneAt(zones, px, py);
    // Arm when the cursor enters any zone; stay armed while inside one. Leaving all
    // zones disarms — the preview disappears until the cursor comes back.
    const armed = zone !== null;
    set({ px, py, zone: zone ?? null, armed });
  },
  end: () => set({ nodeId: null, px: 0, py: 0, armed: false, zone: null }),
}));

/** The zone currently under the cursor, if any (and the preview is armed). */
export function activeSnapZone(): SnapZone | null {
  const { nodeId, zone, armed } = useSnapDragStore.getState();
  if (!nodeId || !armed) return null;
  return zone;
}

/**
 * Finish a snap drag. If the cursor is over a zone, tile the dragged node into it and
 * switch the canvas to tile mode so the arrangement sticks. Otherwise restore the
 * layout that was active before the drag (so a stray drag in tile mode doesn't break
 * the grid). Returns whether it snapped.
 */
export function endSnapDrag(): boolean {
  const { nodeId, px, py, count, prevLayout, prevMode } = useSnapDragStore.getState();
  useSnapDragStore.getState().end();
  if (!nodeId) return false;

  const zone = zoneAt(snapZones(count), px, py);
  if (!zone) {
    // Miss: restore the layout that was active when the drag started, so a stray
    // drag in tile mode doesn't leave the grid half-rearranged.
    useCanvasStore.setState({ layoutMode: prevMode });
    if (prevMode === "tile" && prevLayout) {
      useCanvasStore.setState({ tileLayout: prevLayout });
      useCanvasStore.getState().arrangeTiles();
    }
    return false;
  }

  const canvas = useCanvasStore.getState();
  const others = canvas.nodes.filter((n) => n.id !== nodeId).map((n) => n.id);
  const layout = snapLayout(nodeId, others, zone);
  // Switch to tile mode WITHOUT re-deriving from current positions (setLayout would
  // call retileFromPositions and wipe the snap), then install the snapped arrangement.
  useCanvasStore.setState({ layoutMode: "tile" });
  useCanvasStore.setState({ tileLayout: layout });
  useCanvasStore.getState().arrangeTiles();
  useCanvasStore.getState().focus(nodeId);
  return true;
}
