import { useCanvasStore } from "../stores/canvasStore";
import { type DropTarget } from "./tileTree";
interface SnapDragState {
    nodeId: string | null;
    px: number;
    py: number;
    prevLayout: ReturnType<typeof useCanvasStore.getState>["tileLayout"];
    prevMode: "freeform" | "tile";
    hint: DropTarget | null;
    begin: (nodeId: string) => void;
    move: (px: number, py: number) => void;
    end: () => void;
}
export declare const useSnapDragStore: import("zustand").UseBoundStore<import("zustand").StoreApi<SnapDragState>>;
export declare function activeDropHint(): DropTarget | null;
/** @deprecated use activeDropHint */
export declare function activeSnapZone(): DropTarget | null;
export declare function endSnapDrag(overrideNodeId?: string): boolean;
export {};
//# sourceMappingURL=snapDrag.d.ts.map