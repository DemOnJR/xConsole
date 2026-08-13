import {
  computeTreeBoxes,
  fillOfTree,
  reconcileTree,
  treeFromPositions,
  treeOf,
} from "./tileTree";

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
export const MIN_WEIGHT = 0.25;
export const MAX_WEIGHT = 8;

const clampWeight = (w: number) =>
  Math.min(MAX_WEIGHT, Math.max(MIN_WEIGHT, Number.isFinite(w) ? w : 1));

/** A lone window can be shrunk to a fifth of the pane, no further. */
export const MIN_FILL = 0.2;

const clampFill = (f: number) =>
  Math.min(1, Math.max(MIN_FILL, Number.isFinite(f) ? f : 1));

/** The grid's share of the pane, defaulting to all of it. */
export const fillOf = (layout: TileLayout) => ({
  w: clampFill(layout.fillW ?? 1),
  h: clampFill(layout.fillH ?? 1),
});

/** True when the layout holds exactly one window, i.e. nothing to trade space with. */
export function isSolo(layout: TileLayout): boolean {
  if (layout.columns) {
    return (
      layout.columns.length === 1 && layout.columns[0].items.length === 1
    );
  }
  return layout.rows.length === 1 && layout.rows[0].items.length === 1;
}

/**
 * Resize the lone window by a fraction of the pane on each axis.
 *
 * A no-op for anything but a single-window layout, so the caller can apply it
 * unconditionally alongside [`resizeTile`] / [`resizeRow`].
 */
export function resizeSolo(
  layout: TileLayout,
  dwFraction: number,
  dhFraction: number,
): TileLayout {
  if (!isSolo(layout)) return layout;
  const fill = fillOf(layout);
  return {
    ...layout,
    fillW: clampFill(fill.w + dwFraction),
    fillH: clampFill(fill.h + dhFraction),
  };
}

/**
 * How many tiles each row gets, by default, for `n` nodes.
 *
 * Uses a square-ish grid (`cols = ceil(sqrt(n))`, `rows = ceil(n / cols)`) but then
 * spreads the nodes *evenly across the rows it actually needs*, extras going to the
 * top. That is what makes 3 → `[2, 1]` (bottom tile spans the full width) and
 * 5 → `[3, 2]`, instead of the old square grid that left a hole in the last row.
 */
export function defaultRowCounts(n: number): number[] {
  if (n <= 0) return [];
  const cols = Math.ceil(Math.sqrt(n));
  const rows = Math.ceil(n / cols);
  const base = Math.floor(n / rows);
  const extra = n % rows; // the first `extra` rows carry one more
  return Array.from({ length: rows }, (_, r) => base + (r < extra ? 1 : 0));
}

/** Build a layout that places `ids` into rows of the given sizes, in order. */
export function layoutFromCounts(ids: string[], counts: number[]): TileLayout {
  const rows: TileRow[] = [];
  let i = 0;
  for (const count of counts) {
    if (count <= 0) continue;
    const items = ids.slice(i, i + count).map((id) => ({ id, weight: 1 }));
    i += count;
    if (items.length > 0) rows.push({ weight: 1, items });
  }
  // Anything left over (counts summed to less than ids.length) joins the last row
  // rather than vanishing.
  if (i < ids.length) {
    const rest = ids.slice(i).map((id) => ({ id, weight: 1 }));
    if (rows.length > 0) rows[rows.length - 1].items.push(...rest);
    else rows.push({ weight: 1, items: rest });
  }
  const layout = { rows };
  return { ...layout, tree: treeOf(layout) };
}

/** The balanced layout for a set of nodes — what "Auto" gives you. */
export function autoLayout(ids: string[]): TileLayout {
  const layout = layoutFromCounts(ids, defaultRowCounts(ids.length));
  return { ...layout, tree: treeOf(layout) };
}

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
export function rowsFromPositions(nodes: PlacedNode[]): TileLayout {
  if (nodes.length === 0) return { rows: [] };

  // Detect a column arrangement first: nodes grouped into distinct x-bands (each band
  // is a vertical stack sharing roughly the same x). A column layout looks like
  // [2 on left, 1 tall on right] — the x-centres cluster into groups. Only treat it as
  // columns when there is more than one x-band AND the bands are clearly separated
  // (each band's x-range doesn't overlap the next), so a normal row layout (all nodes
  // sharing one band) stays rows.
  const byX = [...nodes].sort((a, b) => a.x - b.x);
  const bands: PlacedNode[][] = [];
  let current: PlacedNode[] = [];
  let bandRight = 0;
  for (const node of byX) {
    if (current.length === 0) {
      current = [node];
      bandRight = node.x + node.width;
      continue;
    }
    // Overlap test: the node starts before the current band ends (with a little slack).
    if (node.x < bandRight - 1) {
      current.push(node);
      bandRight = Math.max(bandRight, node.x + node.width);
    } else {
      bands.push(current);
      current = [node];
      bandRight = node.x + node.width;
    }
  }
  if (current.length > 0) bands.push(current);

  // Only column when there are 2+ separated bands AND each band stacks 1+ nodes.
  if (bands.length >= 2) {
    const columns = bands.map((band) => {
      const ordered = [...band].sort(
        (a, b) => a.y + a.height / 2 - (b.y + b.height / 2),
      );
      const meanHeight = ordered.reduce((s, n) => s + n.height, 0) / ordered.length || 1;
      return {
        weight: 1,
        items: ordered.map((n) => ({ id: n.id, weight: n.height / meanHeight })),
      };
    });
    // Mirror rows so the row-based helpers (rowCounts, etc.) stay consistent.
    const flat = columns.flatMap((c) => c.items);
    return normalize({
      rows: flat.length > 0 ? [{ weight: 1, items: flat }] : [],
      columns,
    });
  }

  const byTop = [...nodes].sort(
    (a, b) => a.y + a.height / 2 - (b.y + b.height / 2) || a.x - b.x,
  );

  const bandsY: PlacedNode[][] = [];
  let cur: PlacedNode[] = [];
  let meanCentre = 0;
  let meanHeight = 0;

  for (const node of byTop) {
    const centre = node.y + node.height / 2;
    if (cur.length === 0) {
      cur = [node];
      meanCentre = centre;
      meanHeight = node.height;
      continue;
    }
    // Half the mean height is forgiving enough for hand-dragged tiles that are a little
    // off, and tight enough that a deliberately separate row stays separate.
    if (Math.abs(centre - meanCentre) <= Math.max(meanHeight, 1) / 2) {
      cur.push(node);
      meanCentre = cur.reduce((s, n) => s + n.y + n.height / 2, 0) / cur.length;
      meanHeight = cur.reduce((s, n) => s + n.height, 0) / cur.length;
    } else {
      bandsY.push(cur);
      cur = [node];
      meanCentre = centre;
      meanHeight = node.height;
    }
  }
  if (cur.length > 0) bandsY.push(cur);

  const meanRowHeight =
    bandsY.reduce((s, r) => s + r.reduce((m, n) => Math.max(m, n.height), 0), 0) /
    Math.max(bandsY.length, 1);

  const tree = treeFromPositions(nodes);
  return normalize({
    tree,
    rows: bandsY.map((band) => {
      const ordered = [...band].sort(
        (a, b) => a.x + a.width / 2 - (b.x + b.width / 2),
      );
      const meanWidth = ordered.reduce((s, n) => s + n.width, 0) / ordered.length || 1;
      const rowHeight = ordered.reduce((m, n) => Math.max(m, n.height), 0);
      return {
        weight: meanRowHeight > 0 ? rowHeight / meanRowHeight : 1,
        items: ordered.map((n) => ({ id: n.id, weight: n.width / meanWidth })),
      };
    }),
  });
}

/** Every node id in a layout, top-to-bottom then left-to-right (columns), or
 *  top-to-bottom then left-to-right (rows). */
export function layoutIds(layout: TileLayout): string[] {
  if (layout.columns) {
    return layout.columns.flatMap((c) => c.items.map((it) => it.id));
  }
  return layout.rows.flatMap((r) => r.items.map((it) => it.id));
}

/** The row/column of a node, or null if it isn't in the layout. */
export function findTile(
  layout: TileLayout,
  id: string,
): { row: number; col: number } | null {
  if (layout.columns) {
    for (let c = 0; c < layout.columns.length; c++) {
      const idx = layout.columns[c].items.findIndex((it) => it.id === id);
      if (idx !== -1) return { row: idx, col: c };
    }
    return null;
  }
  for (let row = 0; row < layout.rows.length; row++) {
    const col = layout.rows[row].items.findIndex((it) => it.id === id);
    if (col !== -1) return { row, col };
  }
  return null;
}

/** Drop empty rows (and columns) and normalise weights that drifted out of range. */
export function normalize(layout: TileLayout): TileLayout {
  const next: TileLayout = {
    ...layout,
    rows: layout.rows
      .filter((r) => r.items.length > 0)
      .map((r) => ({
        weight: clampWeight(r.weight),
        items: r.items.map((it) => ({ id: it.id, weight: clampWeight(it.weight) })),
      })),
  };
  if (layout.columns) {
    next.columns = layout.columns
      .filter((c) => c.items.length > 0)
      .map((c) => ({
        weight: clampWeight(c.weight),
        items: c.items.map((it) => ({ id: it.id, weight: clampWeight(it.weight) })),
      }));
  }
  return next;
}

/**
 * Reconcile a saved layout with the live node set: drop ids that are gone, append ids
 * that appeared. New nodes join the row with the fewest tiles (ties → the last such
 * row), which keeps an added terminal from lopsiding the grid.
 *
 * Returns the auto layout when nothing was laid out before, so callers can always
 * treat the result as authoritative.
 */
export function reconcile(layout: TileLayout | null, ids: string[]): TileLayout {
  if (!layout || layout.rows.length === 0) return autoLayout(ids);

  const live = new Set(ids);
  const kept: TileRow[] = layout.rows
    .map((r) => ({ weight: r.weight, items: r.items.filter((it) => live.has(it.id)) }))
    .filter((r) => r.items.length > 0);

  if (kept.length === 0) return autoLayout(ids);

  const placed = new Set(kept.flatMap((r) => r.items.map((it) => it.id)));
  for (const id of ids) {
    if (placed.has(id)) continue;
    let target = 0;
    for (let r = 1; r < kept.length; r++) {
      if (kept[r].items.length <= kept[target].items.length) target = r;
    }
    kept[target].items.push({ id, weight: 1 });
    placed.add(id);
  }
  const next = normalize({ ...layout, rows: kept });
  return { ...next, tree: reconcileTree(layout.tree ?? treeOf(next), ids) };
}

/**
 * Turn a layout into pixel boxes filling `width` × `height` exactly.
 *
 * Rounding remainders are absorbed by the last row and the last tile of each row, so
 * the tiles meet edge-to-edge and the grid ends flush with the pane — no seam and no
 * one-pixel gutter, whatever the weights are.
 */
export function computeBoxes(
  layout: TileLayout,
  paneWidth: number,
  paneHeight: number,
): TileBox[] {
  if (paneWidth <= 0 || paneHeight <= 0) return [];

  if (layout.tree) {
    const fill = layout.tree.kind === "leaf" ? fillOfTree(layout) : { w: 1, h: 1 };
    const width = Math.max(1, Math.round(paneWidth * fill.w));
    const height = Math.max(1, Math.round(paneHeight * fill.h));
    return computeTreeBoxes(layout.tree, 0, 0, width, height);
  }

  if (layout.columns) {
    return computeColumnBoxes(layout, paneWidth, paneHeight);
  }

  const rows = layout.rows.filter((r) => r.items.length > 0);
  if (rows.length === 0) return [];

  // A lone window may occupy less than the whole pane; everything else fills it. The
  // stored fraction is kept rather than reset, so closing back down to one window
  // returns it to the size it was left at.
  const fill = isSolo(layout) ? fillOf(layout) : { w: 1, h: 1 };
  const width = Math.max(1, Math.round(paneWidth * fill.w));
  const height = Math.max(1, Math.round(paneHeight * fill.h));

  const boxes: TileBox[] = [];
  const totalRowWeight = rows.reduce((sum, r) => sum + clampWeight(r.weight), 0);

  let y = 0;
  rows.forEach((row, rIdx) => {
    const last = rIdx === rows.length - 1;
    const h = last
      ? height - y // absorb the remainder; also guarantees the grid ends at `height`
      : Math.max(1, Math.floor((height * clampWeight(row.weight)) / totalRowWeight));

    const totalItemWeight = row.items.reduce((sum, it) => sum + clampWeight(it.weight), 0);
    let x = 0;
    row.items.forEach((item, cIdx) => {
      const lastCol = cIdx === row.items.length - 1;
      const w = lastCol
        ? width - x
        : Math.max(1, Math.floor((width * clampWeight(item.weight)) / totalItemWeight));
      boxes.push({ id: item.id, x, y, width: w, height: h });
      x += w;
    });

    y += h;
  });

  return boxes;
}

/**
 * Column-based boxes: each column is a vertical pane sharing the grid width, and the
 * column's items stack top-to-bottom inside it. Mirrors `computeBoxes`' row math, one
 * axis over, so columns meet edge-to-edge and each stack ends flush with the pane.
 */
function computeColumnBoxes(
  layout: TileLayout,
  paneWidth: number,
  paneHeight: number,
): TileBox[] {
  const columns = layout.columns!.filter((c) => c.items.length > 0);
  if (columns.length === 0) return [];

  const fill = isSolo(layout) ? fillOf(layout) : { w: 1, h: 1 };
  const width = Math.max(1, Math.round(paneWidth * fill.w));
  const height = Math.max(1, Math.round(paneHeight * fill.h));

  const boxes: TileBox[] = [];
  const totalColWeight = columns.reduce((sum, c) => sum + clampWeight(c.weight), 0);

  let x = 0;
  columns.forEach((col, cIdx) => {
    const last = cIdx === columns.length - 1;
    const w = last
      ? width - x
      : Math.max(1, Math.floor((width * clampWeight(col.weight)) / totalColWeight));

    const totalItemWeight = col.items.reduce((sum, it) => sum + clampWeight(it.weight), 0);
    let y = 0;
    col.items.forEach((item, rIdx) => {
      const lastRow = rIdx === col.items.length - 1;
      const h = lastRow
        ? height - y
        : Math.max(1, Math.floor((height * clampWeight(item.weight)) / totalItemWeight));
      boxes.push({ id: item.id, x, y, width: w, height: h });
      y += h;
    });

    x += w;
  });

  return boxes;
}

// ---------------------------------------------------------------------------
// Editing operations. Each returns a NEW layout and never mutates its input; each
// is a no-op (returns the original) when the move doesn't apply, so callers can bind
// them straight to a key without guarding.
// ---------------------------------------------------------------------------

const cloneRows = (layout: TileLayout): TileRow[] =>
  layout.rows.map((r) => ({ weight: r.weight, items: r.items.map((it) => ({ ...it })) }));

const cloneColumns = (layout: TileLayout): TileColumn[] =>
  layout.columns!.map((c) => ({ weight: c.weight, items: c.items.map((it) => ({ ...it })) }));

/** Swap a tile with its neighbour in the same row (or column, in a column layout). */
export function moveWithinRow(
  layout: TileLayout,
  id: string,
  dir: -1 | 1,
): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;

  if (layout.columns) {
    // In a column layout "within row" means: move within the same COLUMN (up/down),
    // since the visual rows are the column stacks. `at.row` is the item index, `at.col`
    // the column.
    const columns = cloneColumns(layout);
    const items = columns[at.col].items;
    const to = at.row + dir;
    if (to < 0 || to >= items.length) return layout;
    [items[at.row], items[to]] = [items[to], items[at.row]];
    return { ...layout, columns };
  }

  const rows = cloneRows(layout);
  const items = rows[at.row].items;
  const to = at.col + dir;
  if (to < 0 || to >= items.length) return layout;
  [items[at.col], items[to]] = [items[to], items[at.col]];
  return { ...layout, rows };
}

/**
 * Move a tile to the row above/below (or column left/right in a column layout),
 * keeping roughly its position. Moving past the first/last creates a new one.
 */
export function moveToRow(layout: TileLayout, id: string, dir: -1 | 1): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;

  if (layout.columns) {
    // Column layout: "vertical move" is really a move BETWEEN columns (left/right).
    const columns = cloneColumns(layout);
    const from = columns[at.col];
    // Refuse to empty a column by moving its only tile into a brand-new one.
    const target = at.col + dir;
    const creating = target < 0 || target >= columns.length;
    if (creating && from.items.length === 1) return layout;

    const [tile] = from.items.splice(at.row, 1);
    if (creating) {
      const col: TileColumn = { weight: 1, items: [tile] };
      if (target < 0) columns.unshift(col);
      else columns.push(col);
    } else {
      const dest = columns[target];
      const pos = Math.min(at.row, dest.items.length);
      dest.items.splice(pos, 0, tile);
    }
    return normalize({ ...layout, columns });
  }

  const rows = cloneRows(layout);
  const from = rows[at.row];
  // Refuse to empty a row by moving its only tile into a brand-new row — that would
  // just swap which row is empty.
  const target = at.row + dir;
  const creating = target < 0 || target >= rows.length;
  if (creating && from.items.length === 1) return layout;

  const [tile] = from.items.splice(at.col, 1);
  if (creating) {
    const row: TileRow = { weight: 1, items: [tile] };
    if (target < 0) rows.unshift(row);
    else rows.push(row);
  } else {
    const dest = rows[target];
    const col = Math.min(at.col, dest.items.length);
    dest.items.splice(col, 0, tile);
  }
  return normalize({ ...layout, rows });
}

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
export function resizeTile(layout: TileLayout, id: string, delta: number): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;

  if (layout.columns) {
    // Column layout: a horizontal edge is BETWEEN columns, so the drag changes the
    // column's width weight, trading with the adjacent column.
    const columns = cloneColumns(layout);
    if (columns.length < 2) return layout;
    const neighbourCol = at.col + 1 < columns.length ? at.col + 1 : at.col - 1;
    const col = columns[at.col];
    const neighbour = columns[neighbourCol];

    const before = col.weight;
    col.weight = clampWeight(col.weight + delta);
    const applied = col.weight - before;
    const neighbourAfter = clampWeight(neighbour.weight - applied);
    const absorbed = neighbour.weight - neighbourAfter;
    col.weight = before + absorbed;
    neighbour.weight = neighbourAfter;
    return { ...layout, columns };
  }

  const rows = cloneRows(layout);
  const items = rows[at.row].items;
  if (items.length < 2) return layout;

  const neighbourCol = at.col + 1 < items.length ? at.col + 1 : at.col - 1;
  const item = items[at.col];
  const neighbour = items[neighbourCol];

  // Clamp to what the neighbour can actually give, so the pair's total is unchanged and
  // the rest of the row never moves.
  const before = item.weight;
  item.weight = clampWeight(item.weight + delta);
  const applied = item.weight - before;
  const neighbourAfter = clampWeight(neighbour.weight - applied);
  // The neighbour hit its own floor: give back whatever it could not absorb.
  const absorbed = neighbour.weight - neighbourAfter;
  item.weight = before + absorbed;
  neighbour.weight = neighbourAfter;
  return { ...layout, rows };
}

/**
 * Grow/shrink the height share of a tile's row, taking it from the adjacent row.
 *
 * Same rule as [`resizeTile`], one axis over: a horizontal edge is between two rows, so
 * only those two change height.
 */
export function resizeRow(layout: TileLayout, id: string, delta: number): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;

  if (layout.columns) {
    // Column layout: a vertical edge is between two items INSIDE the same column, so
    // the drag changes the item's height weight, trading with its neighbor in-stack.
    const columns = cloneColumns(layout);
    const items = columns[at.col].items;
    if (items.length < 2) return layout;

    const neighbourRow = at.row + 1 < items.length ? at.row + 1 : at.row - 1;
    const item = items[at.row];
    const neighbour = items[neighbourRow];

    const before = item.weight;
    item.weight = clampWeight(item.weight + delta);
    const applied = item.weight - before;
    const neighbourAfter = clampWeight(neighbour.weight - applied);
    const absorbed = neighbour.weight - neighbourAfter;
    item.weight = before + absorbed;
    neighbour.weight = neighbourAfter;
    return { ...layout, columns };
  }

  const rows = cloneRows(layout);
  if (rows.length < 2) return layout;

  const neighbourRow = at.row + 1 < rows.length ? at.row + 1 : at.row - 1;
  const row = rows[at.row];
  const neighbour = rows[neighbourRow];

  const before = row.weight;
  row.weight = clampWeight(row.weight + delta);
  const applied = row.weight - before;
  const neighbourAfter = clampWeight(neighbour.weight - applied);
  const absorbed = neighbour.weight - neighbourAfter;
  row.weight = before + absorbed;
  neighbour.weight = neighbourAfter;
  return { ...layout, rows };
}

/**
 * Give a tile its own full-width row, or put it back with its neighbours.
 *
 * This is the "make the bottom one full width" key. When the tile already has its row
 * to itself it merges back into the row above (or below, for the first row), so the
 * same key toggles.
 */
export function toggleFullWidth(layout: TileLayout, id: string): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;

  if (layout.columns) {
    // Column layout: "full width" means its own full-height column.
    const columns = cloneColumns(layout);
    if (columns[at.col].items.length === 1) {
      // Already alone in its column — merge back into an adjacent column.
      if (columns.length === 1) return layout;
      const [tile] = columns[at.col].items.splice(0, 1);
      const into = at.col > 0 ? at.col - 1 : 1;
      columns[into].items.push(tile);
      return normalize({ ...layout, columns });
    }
    const [tile] = columns[at.col].items.splice(at.row, 1);
    columns.splice(at.col + 1, 0, { weight: 1, items: [tile] });
    return normalize({ ...layout, columns });
  }

  const rows = cloneRows(layout);

  if (rows[at.row].items.length === 1) {
    // Already alone — merge back into an adjacent row.
    if (rows.length === 1) return layout;
    const [tile] = rows[at.row].items.splice(0, 1);
    const into = at.row > 0 ? at.row - 1 : 1;
    rows[into].items.push(tile);
    return normalize({ ...layout, rows });
  }

  const [tile] = rows[at.row].items.splice(at.col, 1);
  rows.splice(at.row + 1, 0, { weight: 1, items: [tile] });
  return normalize({ ...layout, rows });
}

/** Re-flow the whole layout into rows of the given sizes, keeping the current order. */
export function applyRowCounts(layout: TileLayout, counts: number[]): TileLayout {
  return layoutFromCounts(layoutIds(layout), counts);
}

/** The current shape, e.g. `[3, 2]` — used by the UI and to round-trip a preset. */
export function rowCounts(layout: TileLayout): number[] {
  return layout.rows.map((r) => r.items.length);
}

/**
 * Build a column layout: `counts` are the number of tiles stacked in each column,
 * left to right, so `[2, 1]` is two stacked on the left and one full-height on the
 * right — a sidebar arrangement. `rows` is derived to mirror the same node order, so
 * the row-based edit operations stay meaningful on a column layout.
 */
export function layoutFromColumnCounts(
  ids: string[],
  counts: number[],
): TileLayout {
  const columns: TileColumn[] = [];
  let i = 0;
  for (const count of counts) {
    if (count <= 0) continue;
    const items = ids.slice(i, i + count).map((id) => ({ id, weight: 1 }));
    i += count;
    if (items.length > 0) columns.push({ weight: 1, items });
  }
  // Anything left over joins the last column rather than vanishing.
  if (i < ids.length) {
    const rest = ids.slice(i).map((id) => ({ id, weight: 1 }));
    if (columns.length > 0) columns[columns.length - 1].items.push(...rest);
    else columns.push({ weight: 1, items: rest });
  }
  const rows = columns.flatMap((c) => c.items);
  const layout = normalize({
    rows: rows.length > 0 ? [{ weight: 1, items: rows }] : [],
    columns,
  });
  return { ...layout, tree: treeOf(layout) };
}

/** The current column shape, e.g. `[2, 1]` — empty when the layout is row-based. */
export function columnCounts(layout: TileLayout): number[] {
  return layout.columns ? layout.columns.map((c) => c.items.length) : [];
}

/**
 * Parse a user-typed shape such as `"2|1"` or `"2 / 1"` for columns (the `|` separates
 * columns; a plain `,` is also accepted since a column of one tile is just a stack).
 * Returns null when the text isn't a usable shape.
 */
export function parseColumnCounts(text: string): number[] | null {
  const parts = text
    .split(/[|/]+/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return null;
  const counts: number[] = [];
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return null;
    const n = Number(p);
    if (n <= 0 || n > 64) return null;
    counts.push(n);
  }
  return counts;
}

/**
 * Parse a user-typed shape such as `"3,2"` or `"3 2"`. Returns null when the text
 * isn't a usable shape, so the UI can just show the field as invalid.
 */
export function parseRowCounts(text: string): number[] | null {
  const parts = text
    .split(/[\s,x*/+-]+/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return null;
  const counts: number[] = [];
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return null;
    const n = Number(p);
    if (n <= 0 || n > 64) return null;
    counts.push(n);
  }
  return counts;
}
