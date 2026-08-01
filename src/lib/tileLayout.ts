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
  rows: TileRow[];
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
  return { rows };
}

/** The balanced layout for a set of nodes — what "Auto" gives you. */
export function autoLayout(ids: string[]): TileLayout {
  return layoutFromCounts(ids, defaultRowCounts(ids.length));
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

  const byTop = [...nodes].sort(
    (a, b) => a.y + a.height / 2 - (b.y + b.height / 2) || a.x - b.x,
  );

  const bands: PlacedNode[][] = [];
  let current: PlacedNode[] = [];
  let meanCentre = 0;
  let meanHeight = 0;

  for (const node of byTop) {
    const centre = node.y + node.height / 2;
    if (current.length === 0) {
      current = [node];
      meanCentre = centre;
      meanHeight = node.height;
      continue;
    }
    // Half the mean height is forgiving enough for hand-dragged tiles that are a little
    // off, and tight enough that a deliberately separate row stays separate.
    if (Math.abs(centre - meanCentre) <= Math.max(meanHeight, 1) / 2) {
      current.push(node);
      meanCentre = current.reduce((s, n) => s + n.y + n.height / 2, 0) / current.length;
      meanHeight = current.reduce((s, n) => s + n.height, 0) / current.length;
    } else {
      bands.push(current);
      current = [node];
      meanCentre = centre;
      meanHeight = node.height;
    }
  }
  if (current.length > 0) bands.push(current);

  const meanRowHeight =
    bands.reduce((s, r) => s + r.reduce((m, n) => Math.max(m, n.height), 0), 0) /
    Math.max(bands.length, 1);

  return normalize({
    rows: bands.map((band) => {
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

/** Every node id in a layout, top-to-bottom then left-to-right. */
export function layoutIds(layout: TileLayout): string[] {
  return layout.rows.flatMap((r) => r.items.map((it) => it.id));
}

/** The row/column of a node, or null if it isn't in the layout. */
export function findTile(
  layout: TileLayout,
  id: string,
): { row: number; col: number } | null {
  for (let row = 0; row < layout.rows.length; row++) {
    const col = layout.rows[row].items.findIndex((it) => it.id === id);
    if (col !== -1) return { row, col };
  }
  return null;
}

/** Drop empty rows and normalise weights that drifted out of range. */
export function normalize(layout: TileLayout): TileLayout {
  return {
    ...layout,
    rows: layout.rows
      .filter((r) => r.items.length > 0)
      .map((r) => ({
        weight: clampWeight(r.weight),
        items: r.items.map((it) => ({ id: it.id, weight: clampWeight(it.weight) })),
      })),
  };
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
  return normalize({ ...layout, rows: kept });
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
  const rows = layout.rows.filter((r) => r.items.length > 0);
  if (rows.length === 0 || paneWidth <= 0 || paneHeight <= 0) return [];

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

// ---------------------------------------------------------------------------
// Editing operations. Each returns a NEW layout and never mutates its input; each
// is a no-op (returns the original) when the move doesn't apply, so callers can bind
// them straight to a key without guarding.
// ---------------------------------------------------------------------------

const cloneRows = (layout: TileLayout): TileRow[] =>
  layout.rows.map((r) => ({ weight: r.weight, items: r.items.map((it) => ({ ...it })) }));

/** Swap a tile with its neighbour in the same row. */
export function moveWithinRow(
  layout: TileLayout,
  id: string,
  dir: -1 | 1,
): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;
  const rows = cloneRows(layout);
  const items = rows[at.row].items;
  const to = at.col + dir;
  if (to < 0 || to >= items.length) return layout;
  [items[at.col], items[to]] = [items[to], items[at.col]];
  return { ...layout, rows };
}

/**
 * Move a tile to the row above/below, keeping roughly its horizontal position. Moving
 * past the first/last row creates a new row, so a user can always peel a tile off into
 * its own full-width row.
 */
export function moveToRow(layout: TileLayout, id: string, dir: -1 | 1): TileLayout {
  const at = findTile(layout, id);
  if (!at) return layout;
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
