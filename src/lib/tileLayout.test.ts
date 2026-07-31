import { describe, expect, it } from "vitest";
import {
  autoLayout,
  computeBoxes,
  resizeRow,
  resizeTile,
  type TileLayout,
} from "./tileLayout";

const PANE = { width: 1600, height: 900 };

/** Every tile, keyed by id, for a pane-sized layout. */
function boxes(layout: TileLayout, dims = PANE) {
  return new Map(
    computeBoxes(layout, dims.width, dims.height).map((b) => [b.id, b]),
  );
}

function ids(n: number) {
  return Array.from({ length: n }, (_, i) => `n${i}`);
}

describe("tiles fill the pane", () => {
  // The regression this file exists for: tiles are supposed to be edge to edge. A code
  // path that laid them out without the pane size flowed them with a 24px gap instead,
  // which showed up as "the gap between the windows is too much".
  it.each([1, 2, 3, 5, 8])("leaves no gaps or overhang with %i tiles", (n) => {
    const layout = autoLayout(ids(n));
    const all = computeBoxes(layout, PANE.width, PANE.height);
    expect(all).toHaveLength(n);

    // Rows: group by y, then check each row spans the full width with touching tiles.
    const rows = new Map<number, typeof all>();
    for (const b of all) {
      const row = rows.get(b.y) ?? [];
      row.push(b);
      rows.set(b.y, row);
    }
    for (const row of rows.values()) {
      row.sort((a, b) => a.x - b.x);
      expect(row[0].x).toBe(0);
      for (let i = 1; i < row.length; i += 1) {
        // Touching exactly — no gap, no overlap.
        expect(row[i].x).toBe(row[i - 1].x + row[i - 1].width);
      }
      const lastTile = row[row.length - 1];
      expect(lastTile.x + lastTile.width).toBe(PANE.width);
    }

    // Rows stack with no vertical gap and reach the bottom.
    const ys = [...rows.keys()].sort((a, b) => a - b);
    expect(ys[0]).toBe(0);
    for (let i = 1; i < ys.length; i += 1) {
      const prev = rows.get(ys[i - 1])![0];
      expect(ys[i]).toBe(prev.y + prev.height);
    }
    const lastRow = rows.get(ys[ys.length - 1])![0];
    expect(lastRow.y + lastRow.height).toBe(PANE.height);
  });
});

describe("resizing trades space with one neighbour", () => {
  it("widening the middle tile of five moves only its right neighbour", () => {
    const layout = autoLayout(ids(5));
    // Put all five in one row so "neighbour" is unambiguous.
    const oneRow: TileLayout = {
      rows: [{ weight: 1, items: ids(5).map((id) => ({ id, weight: 1 })) }],
    };
    void layout;

    const before = boxes(oneRow);
    const after = boxes(resizeTile(oneRow, "n2", 0.4));

    expect(after.get("n2")!.width).toBeGreaterThan(before.get("n2")!.width);
    expect(after.get("n3")!.width).toBeLessThan(before.get("n3")!.width);

    // The untouched tiles keep their exact width. This is the actual complaint:
    // "I enlarge the 3rd and everything shrinks" — only the 4th should.
    for (const id of ["n0", "n1", "n4"]) {
      expect(after.get(id)!.width).toBe(before.get(id)!.width);
    }
  });

  it("the pair's combined width is unchanged, so the row still fills the pane", () => {
    const oneRow: TileLayout = {
      rows: [{ weight: 1, items: ids(4).map((id) => ({ id, weight: 1 })) }],
    };
    const before = boxes(oneRow);
    const after = boxes(resizeTile(oneRow, "n1", 0.3));
    const pairBefore = before.get("n1")!.width + before.get("n2")!.width;
    const pairAfter = after.get("n1")!.width + after.get("n2")!.width;
    // Within a pixel: widths are floored per tile, and the last tile absorbs the
    // remainder, so exact equality is not the right assertion.
    expect(Math.abs(pairAfter - pairBefore)).toBeLessThanOrEqual(1);
  });

  it("the last tile in a row trades leftwards instead", () => {
    const oneRow: TileLayout = {
      rows: [{ weight: 1, items: ids(3).map((id) => ({ id, weight: 1 })) }],
    };
    const before = boxes(oneRow);
    const after = boxes(resizeTile(oneRow, "n2", 0.3));
    expect(after.get("n2")!.width).toBeGreaterThan(before.get("n2")!.width);
    expect(after.get("n1")!.width).toBeLessThan(before.get("n1")!.width);
    expect(after.get("n0")!.width).toBe(before.get("n0")!.width);
  });

  it("a tile alone in its row has nothing to trade with, so nothing moves", () => {
    const solo: TileLayout = { rows: [{ weight: 1, items: [{ id: "n0", weight: 1 }] }] };
    expect(resizeTile(solo, "n0", 0.5)).toEqual(solo);
  });

  it("row heights trade with the adjacent row only", () => {
    const three: TileLayout = {
      rows: [
        { weight: 1, items: [{ id: "a", weight: 1 }] },
        { weight: 1, items: [{ id: "b", weight: 1 }] },
        { weight: 1, items: [{ id: "c", weight: 1 }] },
      ],
    };
    const before = boxes(three);
    const after = boxes(resizeRow(three, "a", 0.4));
    expect(after.get("a")!.height).toBeGreaterThan(before.get("a")!.height);
    expect(after.get("b")!.height).toBeLessThan(before.get("b")!.height);
    expect(after.get("c")!.height).toBe(before.get("c")!.height);
  });

  it("stays gap-free after a resize", () => {
    const oneRow: TileLayout = {
      rows: [{ weight: 1, items: ids(5).map((id) => ({ id, weight: 1 })) }],
    };
    const all = computeBoxes(resizeTile(oneRow, "n2", 0.6), PANE.width, PANE.height);
    all.sort((a, b) => a.x - b.x);
    expect(all[0].x).toBe(0);
    for (let i = 1; i < all.length; i += 1) {
      expect(all[i].x).toBe(all[i - 1].x + all[i - 1].width);
    }
    expect(all[all.length - 1].x + all[all.length - 1].width).toBe(PANE.width);
  });
});
