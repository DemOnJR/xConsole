/**
 * Drag-to-place: highlight the window (or pane edge) under the cursor and, on
 * drop, swap or dock. Other tiles are not reshuffled.
 */
import { create } from "zustand";
import { useCanvasStore } from "../stores/canvasStore";
import { computeBoxes, reconcile, rowsFromPositions } from "./tileLayout";
import { applyDrop, dropTargetAt, treeOf } from "./tileTree";
export const useSnapDragStore = create((set) => ({
    nodeId: null,
    px: 0,
    py: 0,
    prevLayout: null,
    prevMode: "freeform",
    hint: null,
    begin: (nodeId) => {
        const canvas = useCanvasStore.getState();
        set({
            nodeId,
            px: 0,
            py: 0,
            prevLayout: canvas.tileLayout,
            prevMode: canvas.layoutMode,
            hint: null,
        });
    },
    move: (px, py) => {
        const { nodeId } = useSnapDragStore.getState();
        if (!nodeId) {
            set({ px, py, hint: null });
            return;
        }
        const canvas = useCanvasStore.getState();
        const pane = canvas.paneSize;
        if (!pane || pane.width <= 0 || pane.height <= 0) {
            set({ px, py, hint: null });
            return;
        }
        const layout = canvas.layoutMode === "tile" && canvas.tileLayout
            ? reconcile(canvas.tileLayout, canvas.nodes.map((n) => n.id))
            : rowsFromPositions(canvas.nodes.map((n) => ({
                id: n.id,
                x: n.position.x,
                y: n.position.y,
                width: Number(n.width) || 460,
                height: Number(n.height) || 320,
            })));
        const boxes = computeBoxes(layout, pane.width, pane.height);
        const hint = dropTargetAt(boxes, px * pane.width, py * pane.height, pane.width, pane.height, nodeId);
        set({ px, py, hint });
    },
    end: () => set({ nodeId: null, px: 0, py: 0, hint: null }),
}));
export function activeDropHint() {
    const { nodeId, hint } = useSnapDragStore.getState();
    return nodeId ? hint : null;
}
/** @deprecated use activeDropHint */
export function activeSnapZone() {
    return activeDropHint();
}
export function endSnapDrag(overrideNodeId) {
    const { nodeId: storeNodeId, hint, prevLayout, prevMode } = useSnapDragStore.getState();
    const nodeId = overrideNodeId || storeNodeId;
    useSnapDragStore.getState().end();
    if (!nodeId || nodeId === "__new_vps__")
        return false;
    if (!hint) {
        const canvas = useCanvasStore.getState();
        if (prevMode === "tile") {
            useCanvasStore.setState({ layoutMode: "tile", tileLayout: prevLayout });
            canvas.arrangeTiles();
        }
        return false;
    }
    const canvas = useCanvasStore.getState();
    const base = (prevMode === "tile" && prevLayout
        ? reconcile(prevLayout, canvas.nodes.map((n) => n.id))
        : rowsFromPositions(canvas.nodes.map((n) => ({
            id: n.id,
            x: n.position.x,
            y: n.position.y,
            width: Number(n.width) || 460,
            height: Number(n.height) || 320,
        }))));
    const layout = applyDrop({ ...base, tree: base.tree ?? treeOf(base) }, nodeId, hint);
    useCanvasStore.setState({ layoutMode: "tile", tileLayout: layout });
    useCanvasStore.getState().arrangeTiles();
    useCanvasStore.getState().focus(nodeId);
    return true;
}
//# sourceMappingURL=snapDrag.js.map