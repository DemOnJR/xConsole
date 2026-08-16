import { describe, expect, it } from "vitest";
import { computeBoxes } from "./tileLayout";
import {
  applyDrop,
  dockLeaf,
  dropTargetAt,
  swapLeaves,
  treeFromPositions,
  treeIds,
  type Split,
} from "./tileTree";

const PANE = { width: 1600, height: 900 };

function box(id: string, x: number, y: number, w: number, h: number) {
  return { id, x, y, width: w, height: h };
}

describe("treeFromPositions", () => {
  it("reads 3 top + 1 mid + 4 bottom + 1 full-height right", () => {
    const left = 1200;
    const right = 400;
    const nodes = [
      box("a", 0, 0, 400, 200),
      box("b", 400, 0, 400, 200),
      box("c", 800, 0, 400, 200),
      box("d", 0, 200, 1200, 200),
      box("e", 0, 400, 300, 500),
      box("f", 300, 400, 300, 500),
      box("g", 600, 400, 300, 500),
      box("h", 900, 400, 300, 500),
      box("i", left, 0, right, 900),
    ];
    const tree = treeFromPositions(nodes);
    expect(tree.kind).toBe("row");
    const ids = treeIds(tree);
    expect(ids.sort()).toEqual(["a", "b", "c", "d", "e", "f", "g", "h", "i"].sort());
    const row = tree as Extract<Split, { kind: "row" }>;
    expect(row.kids).toHaveLength(2);
    const rightPane = row.kids[1];
    expect(rightPane.kind === "leaf" && rightPane.id === "i").toBe(true);
    const leftSide = row.kids[0];
    expect(leftSide.kind).toBe("col");
    const bands = (leftSide as Extract<Split, { kind: "col" }>).kids;
    expect(bands).toHaveLength(3);
    expect(treeIds(bands[0]).sort()).toEqual(["a", "b", "c"]);
    expect(treeIds(bands[1])).toEqual(["d"]);
    expect(treeIds(bands[2]).sort()).toEqual(["e", "f", "g", "h"]);
  });

  it("reads 1 top + 2 bottom on the left beside a full-height right pane", () => {
    const nodes = [
      box("a", 0, 0, 800, 450),
      box("b", 0, 450, 400, 450),
      box("c", 400, 450, 400, 450),
      box("d", 800, 0, 800, 900),
    ];
    const tree = treeFromPositions(nodes);
    expect(tree.kind).toBe("row");
    const row = tree as Extract<Split, { kind: "row" }>;
    expect(row.kids).toHaveLength(2);
    expect(row.kids[1].kind === "leaf" && row.kids[1].id === "d").toBe(true);
    const left = row.kids[0];
    expect(left.kind).toBe("col");
    const bands = (left as Extract<Split, { kind: "col" }>).kids;
    expect(treeIds(bands[0])).toEqual(["a"]);
    expect(treeIds(bands[1]).sort()).toEqual(["b", "c"]);
  });
});

describe("swapLeaves", () => {
  it("swaps two windows and leaves everyone else in place", () => {
    const nodes = [
      box("a", 0, 0, 800, 450),
      box("b", 800, 0, 800, 450),
      box("c", 0, 450, 800, 450),
      box("d", 800, 450, 800, 450),
    ];
    const layout = { rows: [], tree: treeFromPositions(nodes) };
    const before = new Map(computeBoxes(layout, PANE.width, PANE.height).map((b) => [b.id, b]));
    const after = new Map(
      computeBoxes(swapLeaves(layout, "c", "b"), PANE.width, PANE.height).map((b) => [b.id, b]),
    );
    const pos = (b: { x: number; y: number; width: number; height: number }) => ({
      x: b.x,
      y: b.y,
      width: b.width,
      height: b.height,
    });
    expect(pos(after.get("c")!)).toEqual(pos(before.get("b")!));
    expect(pos(after.get("b")!)).toEqual(pos(before.get("c")!));
    expect(pos(after.get("a")!)).toEqual(pos(before.get("a")!));
    expect(pos(after.get("d")!)).toEqual(pos(before.get("d")!));
  });
});

describe("dockLeaf", () => {
  it("splits one window without reshuffling the others", () => {
    const nodes = [
      box("a", 0, 0, 800, 900),
      box("b", 800, 0, 800, 900),
    ];
    const layout = { rows: [], tree: treeFromPositions(nodes) };
    const next = dockLeaf(layout, "a", "b", "top");
    const after = new Map(computeBoxes(next, PANE.width, PANE.height).map((bx) => [bx.id, bx]));
    expect(after.get("a")!.y).toBeLessThan(after.get("b")!.y);
    expect(after.get("a")!.x).toBe(0);
    expect(after.get("b")!.x).toBe(0);
    expect(after.get("a")!.width).toBe(PANE.width);
  });
});

describe("dropTargetAt", () => {
  it("swaps when the cursor is in the centre of another window", () => {
    const boxes = [
      { id: "a", x: 0, y: 0, width: 800, height: 900 },
      { id: "b", x: 800, y: 0, width: 800, height: 900 },
    ];
    const hit = dropTargetAt(boxes, 1200, 450, 1600, 900, "a");
    expect(hit?.kind).toBe("swap");
    expect(hit?.targetId).toBe("b");
    const layout = { rows: [], tree: treeFromPositions(boxes) };
    const next = applyDrop(layout, "a", hit!);
    const after = computeBoxes(next, 1600, 900);
    const b = after.find((x) => x.id === "b")!;
    expect(b.x).toBe(0);
  });
});
