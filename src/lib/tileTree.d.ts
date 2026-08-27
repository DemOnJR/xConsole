/**
 * Recursive split-tree tiling.
 *
 * Rows-only or columns-only cannot express "3 on top, 1 in the middle, 4 on the
 * bottom, and one full-height pane on the right". A tree can: a row of
 * [column of those bands, right leaf].
 *
 * Drag operations (swap / dock) edit two leaves and leave every other window
 * where it is. Inferring the tree from positions is how Tile becomes automatic.
 */
import type { PlacedNode, TileBox, TileLayout } from "./tileLayout";
export type DockEdge = "left" | "right" | "top" | "bottom";
export type Split = {
    kind: "leaf";
    id: string;
    weight: number;
} | {
    kind: "row";
    weight: number;
    kids: Split[];
} | {
    kind: "col";
    weight: number;
    kids: Split[];
};
export declare function leaf(id: string, weight?: number): Split;
export declare function treeIds(node: Split): string[];
export declare function cloneSplit(node: Split): Split;
/** Collapse single-child splits and drop empty ones. */
export declare function prune(node: Split | null): Split | null;
export declare function treeFromIdsRow(ids: string[]): Split;
/** Balanced default as a column of rows (same shape as autoLayout). */
export declare function autoTree(ids: string[]): Split;
/**
 * Read a split tree from where the windows sit.
 *
 * Vertical gaps (a tall pane on the right) are preferred, then horizontal bands.
 * Each group is solved the same way, so "3 / 1 / 4 + one on the right" falls out
 * without a shape picker.
 */
export declare function treeFromPositions(nodes: PlacedNode[]): Split;
export declare function computeTreeBoxes(node: Split, x: number, y: number, width: number, height: number): TileBox[];
export declare function layoutFromTree(tree: Split, fillW?: number, fillH?: number): TileLayout;
export declare function treeOf(layout: TileLayout): Split;
/** Swap two windows. Every other tile stays put. */
export declare function swapLeaves(layout: TileLayout, a: string, b: string): TileLayout;
/** Dock `dragged` onto an edge of `target`. Other windows keep their places. */
export declare function dockLeaf(layout: TileLayout, dragged: string, target: string, edge: DockEdge): TileLayout;
/** Dock against the outer pane (new split at the root). */
export declare function dockToPane(layout: TileLayout, dragged: string, edge: DockEdge): TileLayout;
export declare function reconcileTree(tree: Split | null, ids: string[]): Split;
/** Trade size with the sibling on the matching axis. */
export declare function resizeTree(layout: TileLayout, id: string, dw: number, dh: number): TileLayout;
export declare function moveInTree(layout: TileLayout, id: string, dir: -1 | 1, axis: "horizontal" | "vertical"): TileLayout;
export type DropKind = "swap" | "dock" | "pane";
export interface DropTarget {
    kind: DropKind;
    targetId?: string;
    edge?: DockEdge;
    /** Highlight rectangle in pane pixels. */
    x: number;
    y: number;
    width: number;
    height: number;
}
export declare function dropTargetAt(boxes: TileBox[], x: number, y: number, paneW: number, paneH: number, draggedId: string): DropTarget | null;
export declare function applyDrop(layout: TileLayout, draggedId: string, drop: DropTarget): TileLayout;
export declare function fillOfTree(layout: TileLayout): {
    w: number;
    h: number;
};
/** Persistable tree (ids as node indices). */
export type SavedSplit = {
    kind: "leaf";
    index: number;
    weight: number;
} | {
    kind: "row" | "col";
    weight: number;
    kids: SavedSplit[];
};
export declare function serializeSplit(node: Split, indexOf: (id: string) => number): SavedSplit | null;
export declare function deserializeSplit(saved: SavedSplit, ids: string[]): Split | null;
//# sourceMappingURL=tileTree.d.ts.map