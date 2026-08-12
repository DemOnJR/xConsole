/**
 * Snap-layout engine: Windows-style tiling zones while dragging a window.
 *
 * The canvas has a fixed flexbox pane (the React Flow viewport). While a node is being
 * dragged by its header, an overlay shows translucent zones (left half, right half,
 * quadrants, 2-1 / 1-2 columns…). Dropping into a zone tiles the dragged node into that
 * slot of the layout and reflows the *other* nodes into the remaining slots, switching
 * the canvas to tile mode so the arrangement sticks.
 *
 * Everything here is pure so it can be unit-tested without React or the store.
 */
import type { TileLayout } from "./tileLayout";

/** A zone is a rectangle of the pane plus the layout shape it maps to. */
export interface SnapZone {
  id: string;
  /** Fractional rectangle of the pane this zone occupies. */
  x: number;
  y: number;
  w: number;
  h: number;
  /** The shape to apply: `[1, n-1]` rows, or `[2, 1]` columns, etc. */
  shape: number[];
  mode: "rows" | "columns";
  /** Which tile slot of the shape the dragged node lands in (0-based). */
  slot: number;
}

/** Normalise a shape so it sums to `n` (extra tiles join the last bucket). */
function normShape(shape: number[], n: number): number[] {
  const base = shape.filter((c) => c > 0);
  const total = base.reduce((a, b) => a + b, 0);
  if (total === n) return base;
  const out = [...base];
  let i = out.length - 1;
  while (out.reduce((a, b) => a + b, 0) < n) {
    out[i] += 1;
    i = (i - 1 + out.length) % out.length;
  }
  return out;
}

/** The linear start index of `slot` in a shape, e.g. shape [2,1] slot 1 → 2. */
function slotStart(shape: number[], slot: number): number {
  return shape.slice(0, slot).reduce((a, b) => a + b, 0);
}

/** Zones for `n` total nodes (the dragged one + the rest). */
export function snapZones(n: number): SnapZone[] {
  if (n <= 0) return [];
  const z: SnapZone[] = [];
  const add = (zone: Omit<SnapZone, "id"> & { id: string }) => {
    // Only offer shapes that actually sum to n.
    if (zone.shape.reduce((a, b) => a + b, 0) !== n) return;
    z.push(zone);
  };

  // Corner strips first so they win the hit-test over the edges that contain them.
  if (n >= 3) {
    add({ id: "tl", x: 0, y: 0, w: 0.25, h: 0.2, shape: [2, n - 2], mode: "rows", slot: 0 });
    add({ id: "tr", x: 0.75, y: 0, w: 0.25, h: 0.2, shape: [2, n - 2], mode: "rows", slot: 1 });
    add({ id: "bl", x: 0, y: 0.8, w: 0.25, h: 0.2, shape: [2, n - 2], mode: "rows", slot: 0 });
    add({ id: "br", x: 0.75, y: 0.8, w: 0.25, h: 0.2, shape: [2, n - 2], mode: "rows", slot: 1 });
  }

  // Edge strips: the Windows-style "drag to the edge" triggers.
  add({ id: "left", x: 0, y: 0, w: 0.2, h: 1, shape: [1, n - 1], mode: "columns", slot: 0 });
  add({ id: "right", x: 0.8, y: 0, w: 0.2, h: 1, shape: [n - 1, 1], mode: "columns", slot: 1 });
  add({ id: "top", x: 0, y: 0, w: 1, h: 0.15, shape: [1, n - 1], mode: "rows", slot: 0 });
  add({ id: "bottom", x: 0, y: 0.85, w: 1, h: 0.15, shape: [n - 1, 1], mode: "rows", slot: 1 });

  // 2-on-left / 1-tall-right (sidebar), and the mirror — triggered near the vertical
  // thirds so they don't collide with the corner/edge zones.
  if (n >= 3) {
    add({ id: "side-left", x: 0.25, y: 0, w: 0.5, h: 1, shape: [2, 1], mode: "columns", slot: 0 });
    add({ id: "side-right", x: 0.25, y: 0, w: 0.5, h: 1, shape: [1, 2], mode: "columns", slot: 1 });
  }

  return z;
}

/** The zone under a pane point, if any. */
export function zoneAt(zones: SnapZone[], px: number, py: number): SnapZone | null {
  for (const zone of zones) {
    if (px >= zone.x && px <= zone.x + zone.w && py >= zone.y && py <= zone.y + zone.h) {
      return zone;
    }
  }
  return null;
}

/**
 * Build the tile layout for a snap: the dragged node goes into `slot` of the zone's
 * shape, the other nodes fill the remaining slots in their current order. Row-based
 * zones produce a row layout; column-based zones produce a column layout.
 */
export function snapLayout(
  draggedId: string,
  otherIds: string[],
  zone: SnapZone,
): TileLayout {
  const n = otherIds.length + 1;
  const shape = normShape(zone.shape, n);
  const start = slotStart(shape, zone.slot);
  // Place the dragged node at the start of its slot's bucket, the others in order.
  const ordered: string[] = [];
  const others = [...otherIds];
  for (let i = 0; i < n; i += 1) {
    if (i === start) ordered.push(draggedId);
    else ordered.push(others.shift()!);
  }

  if (zone.mode === "columns") {
    const columns = shape.map((count) => ({
      weight: 1,
      items: ordered.splice(0, count).map((id) => ({ id, weight: 1 })),
    }));
    const rows = columns.flatMap((c) => c.items);
    return {
      columns,
      rows: rows.length > 0 ? [{ weight: 1, items: rows }] : [],
    };
  }

  const rows = shape.map((count) => ({
    weight: 1,
    items: ordered.splice(0, count).map((id) => ({ id, weight: 1 })),
  }));
  return { rows };
}
