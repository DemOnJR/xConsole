/**
 * Snap-drag state: who is being dragged, where, and the zone under the cursor.
 *
 * The node headers start a snap drag with `beginSnapDrag`; the overlay reads this
 * state to draw the zones and highlight. `endSnapDrag` applies the snapped layout
 * via the canvas store (switching to tile mode) when a zone is hit.
 */
import { create } from "zustand";
import { useCanvasStore } from "../stores/canvasStore";
import { snapZones, zoneAt, snapLayout } from "./snapLayout";

interface SnapDragState {
  /** The node being dragged, or null when idle. */
  nodeId: string | null;
  /** Cursor position in pane coordinates (0..1). */
  px: number;
  py: number;
  /** Total node count when the drag started. */
  count: number;
  begin: (nodeId: string) => void;
  move: (px: number, py: number) => void;
  end: () => void;
}

export const useSnapDragStore = create<SnapDragState>((set) => ({
  nodeId: null,
  px: 0,
  py: 0,
  count: 0,
  begin: (nodeId) =>
    set({ nodeId, px: 0, py: 0, count: useCanvasStore.getState().nodes.length }),
  move: (px, py) => set({ px, py }),
  end: () => set({ nodeId: null, px: 0, py: 0 }),
}));

/** The zone currently under the cursor, if any. */
export function activeSnapZone(): ReturnType<typeof zoneAt> {
  const { nodeId, px, py, count } = useSnapDragStore.getState();
  if (!nodeId) return null;
  return zoneAt(snapZones(count), px, py);
}

/**
 * Finish a snap drag. If the cursor is over a zone, tile the dragged node into it and
 * switch the canvas to tile mode so the arrangement sticks. Returns whether it snapped.
 */
export function endSnapDrag(): boolean {
  const { nodeId, px, py, count } = useSnapDragStore.getState();
  useSnapDragStore.getState().end();
  if (!nodeId) return false;

  const zone = zoneAt(snapZones(count), px, py);
  if (!zone) return false;

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
