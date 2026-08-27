/**
 * Freestyle tiling: the pure geometry layer.
 *
 * The canvas keeps its nodes in one flat array whose *index is part of the saved
 * workspace node id* (`workspaceStore.workspaceNodeId`), so re-ordering that array
 * would silently re-key every live SSH session. Tiling therefore never touches it:
 * the arrangement lives in a separate `TileLayout` that only references node ids.
 *
 * A layout is a list of rows; each row holds an ordered list of node ids. Rows have a
 * height weight and items a width weight, so "3 on top, 2 on bottom, both rows full
 * width" is just `[[a,b,c],[d,e]]`, and a row holding one node spans the full width by
 * construction — no special case.
 *
 * Everything here is pure so it can be unit-tested without React or the store.
 */
/** One tile: a node id plus its share of the row's width. */
export interface TileItem {
    id: string;
    /** Relative width within the row. 1 = an equal share. */
    weight: number;
}
/** One row of tiles, with its share of the pane height. */
export interface TileRow {
    /** Relative height against the other rows. 1 = an equal share. */
    weight: number;
    items: TileItem[];
}
export interface TileLayout {
    /**
     * Nested split tree. When set, boxes / drag / resize use this so a tall pane
     * can sit beside several stacked rows. `rows` / `columns` stay as a flat
     * fallback for older workspaces.
     */
    tree?: import("./tileTree").Split;
    rows: TileRow[];
    /**
     * When set, the layout is **column-based** (side-by-side panes) instead of the
     * row-based model: `columns[c]` is one vertical pane whose items stack top-to-bottom,
     * and the panes share the width of the grid. `rows` is still maintained (it mirrors
     * the same node order) so all the row-based edit operations keep working — they just
     * re-arrange the order the columns then split.
     *
     * A column's items use the same `TileItem` type: the item weight is the height share
     * within the column, and the column weight is the width share against the other
     * columns.
     */
    columns?: TileColumn[];
    /**
     * What fraction of the pane the whole grid occupies, 0.2–1.
     *
     * Resizing normally trades space between two neighbours, so the grid always fills the
     * pane and these stay 1. A *lone* window has no neighbour to trade with, which made it
     * the one window in the app that could not be resized at all: every drag was clamped
     * away and the reflow snapped it straight back to full screen. For that case the drag
     * shrinks the grid itself instead.
     */
    fillW?: number;
    fillH?: number;
}
/** One column of the layout: a vertical pane with a width share and stacked items. */
export interface TileColumn {
    /** Relative width against the other columns. 1 = an equal share. */
    weight: number;
    items: TileItem[];
}
/** A computed pixel box for one node. */
export interface TileBox {
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
}
/** Weights are clamped to this range so a tile can never collapse or eat the row. */
export declare const MIN_WEIGHT = 0.25;
export declare const MAX_WEIGHT = 8;
/** A lone window can be shrunk to a fifth of the pane, no further. */
export declare const MIN_FILL = 0.2;
/** The grid's share of the pane, defaulting to all of it. */
export declare const fillOf: (layout: TileLayout) => {
    w: number;
    h: number;
};
/** True when the layout holds exactly one window, i.e. nothing to trade space with. */
export declare function isSolo(layout: TileLayout): boolean;
/**
 * Resize the lone window by a fraction of the pane on each axis.
 *
 * A no-op for anything but a single-window layout, so the caller can apply it
 * unconditionally alongside [`resizeTile`] / [`resizeRow`].
 */
export declare function resizeSolo(layout: TileLayout, dwFraction: number, dhFraction: number): TileLayout;
/**
 * How many tiles each row gets, by default, for `n` nodes.
 *
 * Uses a square-ish grid (`cols = ceil(sqrt(n))`, `rows = ceil(n / cols)`) but then
 * spreads the nodes *evenly across the rows it actually needs*, extras going to the
 * top. That is what makes 3 → `[2, 1]` (bottom tile spans the full width) and
 * 5 → `[3, 2]`, instead of the old square grid that left a hole in the last row.
 */
export declare function defaultRowCounts(n: number): number[];
/** Build a layout that places `ids` into rows of the given sizes, in order. */
export declare function layoutFromCounts(ids: string[], counts: number[]): TileLayout;
/** The balanced layout for a set of nodes — what "Auto" gives you. */
export declare function autoLayout(ids: string[]): TileLayout;
/** A node as the position reader sees it. */
export interface PlacedNode {
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
}
/**
 * Read a layout back out of where the nodes currently sit.
 *
 * This is what makes "drag them roughly where you want, then press Tile" work: three
 * terminals side by side become one row of three, two-over-one becomes `[2, 1]`, and so
 * on. Without it, Tile can only ever impose its own idea of the arrangement, and
 * dragging is purely cosmetic until the next re-tile wipes it out.
 *
 * Rows are found by banding on the vertical centre rather than the top edge, so tiles
 * that are roughly level still group together even when their heights differ. A node
 * joins the row being built when its centre is within half the row's mean height of the
 * row's mean centre — comparing against the running mean (not a growing envelope) stops
 * a staircase of slightly-offset nodes from chaining into one giant row.
 *
 * Relative sizes carry over too: within a row, width becomes the tile's weight, and a
 * row's height becomes the row's weight. So dragging one terminal wider and then tiling
 * keeps it wider instead of snapping everything back to equal shares.
 */
export declare function rowsFromPositions(nodes: PlacedNode[]): TileLayout;
/** Every node id in a layout, top-to-bottom then left-to-right (columns), or
 *  top-to-bottom then left-to-right (rows). */
export declare function layoutIds(layout: TileLayout): string[];
/** The row/column of a node, or null if it isn't in the layout. */
export declare function findTile(layout: TileLayout, id: string): {
    row: number;
    col: number;
} | null;
/** Drop empty rows (and columns) and normalise weights that drifted out of range. */
export declare function normalize(layout: TileLayout): TileLayout;
/**
 * Reconcile a saved layout with the live node set: drop ids that are gone, append ids
 * that appeared. New nodes join the row with the fewest tiles (ties → the last such
 * row), which keeps an added terminal from lopsiding the grid.
 *
 * Returns the auto layout when nothing was laid out before, so callers can always
 * treat the result as authoritative.
 */
export declare function reconcile(layout: TileLayout | null, ids: string[]): TileLayout;
/**
 * Turn a layout into pixel boxes filling `width` × `height` exactly.
 *
 * Rounding remainders are absorbed by the last row and the last tile of each row, so
 * the tiles meet edge-to-edge and the grid ends flush with the pane — no seam and no
 * one-pixel gutter, whatever the weights are.
 */
export declare function computeBoxes(layout: TileLayout, paneWidth: number, paneHeight: number): TileBox[];
/** Swap a tile with its neighbour in the same row (or column, in a column layout). */
export declare function moveWithinRow(layout: TileLayout, id: string, dir: -1 | 1): TileLayout;
/**
 * Move a tile to the row above/below (or column left/right in a column layout),
 * keeping roughly its position. Moving past the first/last creates a new one.
 */
export declare function moveToRow(layout: TileLayout, id: string, dir: -1 | 1): TileLayout;
/** Grow/shrink a tile's width share within its row. */
/**
 * Grow or shrink a tile, taking the space from **one neighbour**.
 *
 * Weights are normalised across the row when they are turned into pixels, so simply
 * raising one tile's weight shrinks every other tile in the row a little. With five
 * terminals open, widening the third visibly nudged the first, second, fourth and fifth —
 * which is not what dragging an edge means. An edge sits between exactly two tiles, and
 * moving it should trade width between exactly those two.
 *
 * The neighbour is the one on the side the space is coming from: growing takes from the
 * right, shrinking gives back to the right. A tile at the end of the row trades with the
 * tile on its left instead, and a tile alone in its row has nothing to trade with.
 */
export declare function resizeTile(layout: TileLayout, id: string, delta: number): TileLayout;
/**
 * Grow/shrink the height share of a tile's row, taking it from the adjacent row.
 *
 * Same rule as [`resizeTile`], one axis over: a horizontal edge is between two rows, so
 * only those two change height.
 */
export declare function resizeRow(layout: TileLayout, id: string, delta: number): TileLayout;
/**
 * Give a tile its own full-width row, or put it back with its neighbours.
 *
 * This is the "make the bottom one full width" key. When the tile already has its row
 * to itself it merges back into the row above (or below, for the first row), so the
 * same key toggles.
 */
export declare function toggleFullWidth(layout: TileLayout, id: string): TileLayout;
/** Re-flow the whole layout into rows of the given sizes, keeping the current order. */
export declare function applyRowCounts(layout: TileLayout, counts: number[]): TileLayout;
/** The current shape, e.g. `[3, 2]` — used by the UI and to round-trip a preset. */
export declare function rowCounts(layout: TileLayout): number[];
/**
 * Build a column layout: `counts` are the number of tiles stacked in each column,
 * left to right, so `[2, 1]` is two stacked on the left and one full-height on the
 * right — a sidebar arrangement. `rows` is derived to mirror the same node order, so
 * the row-based edit operations stay meaningful on a column layout.
 */
export declare function layoutFromColumnCounts(ids: string[], counts: number[]): TileLayout;
/** The current column shape, e.g. `[2, 1]` — empty when the layout is row-based. */
export declare function columnCounts(layout: TileLayout): number[];
/**
 * Parse a user-typed shape such as `"2|1"` or `"2 / 1"` for columns (the `|` separates
 * columns; a plain `,` is also accepted since a column of one tile is just a stack).
 * Returns null when the text isn't a usable shape.
 */
export declare function parseColumnCounts(text: string): number[] | null;
/**
 * Parse a user-typed shape such as `"3,2"` or `"3 2"`. Returns null when the text
 * isn't a usable shape, so the UI can just show the field as invalid.
 */
export declare function parseRowCounts(text: string): number[] | null;
//# sourceMappingURL=tileLayout.d.ts.map