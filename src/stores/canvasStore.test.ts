import { beforeEach, describe, expect, it } from "vitest";
import { useCanvasStore } from "./canvasStore";
import type { Vps } from "../lib/tauri";

const PANE = { width: 1600, height: 900 };

function vps(n: number): Vps {
  return {
    id: `vps-${n}`,
    name: `server ${n}`,
    host: `10.0.0.${n}`,
    port: 22,
    username: "root",
    auth_type: "key",
    key_path: null,
    tags: null,
    created_at: "",
  } as Vps;
}

/** Tile geometry of the live nodes, left to right. */
function tiles() {
  return useCanvasStore
    .getState()
    .nodes.map((n) => ({
      id: n.id,
      x: n.position.x,
      y: n.position.y,
      w: (n.width as number) ?? 0,
      h: (n.height as number) ?? 0,
    }))
    .sort((a, b) => a.y - b.y || a.x - b.x);
}

/** No gap and no overlap between horizontally adjacent tiles, and the row fills the pane. */
function expectGapFree() {
  const byRow = new Map<number, ReturnType<typeof tiles>>();
  for (const t of tiles()) {
    byRow.set(t.y, [...(byRow.get(t.y) ?? []), t]);
  }
  for (const row of byRow.values()) {
    expect(row[0].x).toBe(0);
    for (let i = 1; i < row.length; i += 1) {
      expect(row[i].x).toBe(row[i - 1].x + row[i - 1].w);
    }
    expect(row[row.length - 1].x + row[row.length - 1].w).toBe(PANE.width);
  }
}

/** Columns fill the pane side by side, and each column's stack meets edge to edge. */
function expectColumnsGapFree() {
  const byCol = new Map<number, ReturnType<typeof tiles>>();
  for (const t of tiles()) {
    byCol.set(t.x, [...(byCol.get(t.x) ?? []), t]);
  }
  const xs = [...byCol.keys()].sort((a, b) => a - b);
  for (const x of xs) {
    const col = byCol.get(x)!;
    expect(col[0].y).toBe(0);
    for (let i = 1; i < col.length; i += 1) {
      expect(col[i].y).toBe(col[i - 1].y + col[i - 1].h);
    }
    expect(col[col.length - 1].y + col[col.length - 1].h).toBe(PANE.height);
  }
  // Columns touch left-to-right.
  for (let i = 1; i < xs.length; i += 1) {
    const prev = byCol.get(xs[i - 1])!;
    const prevRight = Math.max(...prev.map((t) => t.x + t.w));
    expect(xs[i]).toBe(prevRight);
  }
  const last = byCol.get(xs[xs.length - 1])!;
  expect(Math.max(...last.map((t) => t.x + t.w))).toBe(PANE.width);
}

describe("tile mode keeps windows edge to edge", () => {
  beforeEach(() => {
    useCanvasStore.getState().clear();
    useCanvasStore.setState({ layoutMode: "tile", paneSize: null });
  });

  function openThree() {
    const s = useCanvasStore.getState();
    const ids = [s.addVps(vps(1)), s.addVps(vps(2)), s.addVps(vps(3))];
    useCanvasStore.getState().setPaneSize(PANE);
    return ids;
  }

  it("tiles fill the pane once it has been measured", () => {
    openThree();
    expectGapFree();
  });

  /**
   * The regression. Dragging a node edge produced a `dimensions` change, and the handler
   * re-tiled without passing the pane size — which silently took `applyTiles`' fallback
   * path and flowed the rows with a 24px GAP between them instead of filling the pane.
   */
  it("stays gap-free after a resize drag", () => {
    const [, second] = openThree();
    const node = useCanvasStore.getState().nodes.find((n) => n.id === second)!;
    useCanvasStore.getState().onNodesChange([
      {
        id: second,
        type: "dimensions",
        resizing: true,
        dimensions: {
          width: ((node.width as number) ?? 0) + 120,
          height: (node.height as number) ?? 0,
        },
      },
    ]);
    expectGapFree();
  });

  /**
   * React Flow reports its own measurements as `dimensions` changes with no `resizing`
   * flag. On the first one a node has no width yet, so treating it as a resize would read
   * the delta as the node's entire width and fling the layout across the pane.
   */
  it("ignores React Flow's own measurement passes", () => {
    const [first] = openThree();
    const before = tiles();
    useCanvasStore.getState().onNodesChange([
      { id: first, type: "dimensions", dimensions: { width: 640, height: 480 } },
    ]);
    expect(tiles()).toEqual(before);
  });

  it("a resize moves the neighbour, not every other window", () => {
    const ids = openThree();
    // One row, so "neighbour" is unambiguous.
    useCanvasStore.getState().setTileLayout({
      rows: [{ weight: 1, items: ids.map((id) => ({ id, weight: 1 })) }],
    });
    useCanvasStore.getState().arrangeTiles(PANE);
    const before = new Map(tiles().map((t) => [t.id, t]));

    const node = useCanvasStore.getState().nodes.find((n) => n.id === ids[0])!;
    useCanvasStore.getState().onNodesChange([
      {
        id: ids[0],
        type: "dimensions",
        resizing: true,
        dimensions: {
          width: ((node.width as number) ?? 0) + 200,
          height: (node.height as number) ?? 0,
        },
      },
    ]);

    const after = new Map(tiles().map((t) => [t.id, t]));
    expect(after.get(ids[0])!.w).toBeGreaterThan(before.get(ids[0])!.w);
    expect(after.get(ids[1])!.w).toBeLessThan(before.get(ids[1])!.w);
    // The third window must not have moved or changed size.
    expect(after.get(ids[2])!.w).toBe(before.get(ids[2])!.w);
    expectGapFree();
  });
});

describe("column tiling (side-by-side panes)", () => {
  beforeEach(() => {
    useCanvasStore.getState().clear();
    useCanvasStore.setState({ layoutMode: "tile", paneSize: null });
  });

  it("splits into two balanced columns: 2 stacked left, 1 tall right", () => {
    const s = useCanvasStore.getState();
    const ids = [s.addVps(vps(1)), s.addVps(vps(2)), s.addVps(vps(3))];
    useCanvasStore.getState().setPaneSize(PANE);
    useCanvasStore.getState().setTileColumns([2, 1]);
    expectColumnsGapFree();

    const byId = new Map(tiles().map((t) => [t.id, t]));
    // Left column: the first two stack, same x, full height split.
    const a = byId.get(ids[0])!;
    const b = byId.get(ids[1])!;
    const c = byId.get(ids[2])!;
    expect(a.x).toBe(0);
    expect(b.x).toBe(0);
    expect(c.x).toBe(PANE.width / 2);
    expect(a.w).toBe(PANE.width / 2);
    expect(c.w).toBe(PANE.width / 2);
    expect(a.h).toBe(PANE.height / 2);
    expect(b.h).toBe(PANE.height / 2);
    expect(c.h).toBe(PANE.height);
  });

  it("keeps the arrangement after a resize drag (columns stay gap-free)", () => {
    const s = useCanvasStore.getState();
    const ids = [s.addVps(vps(1)), s.addVps(vps(2)), s.addVps(vps(3))];
    useCanvasStore.getState().setPaneSize(PANE);
    useCanvasStore.getState().setTileColumns([2, 1]);
    const layoutBefore = useCanvasStore.getState().tileLayout;
    expect(layoutBefore?.columns).toBeTruthy();

    // Resize the agent-ish third node (the one alone in the right column) — the
    // reported bug: this used to drop the columns and revert to row layout.
    const node = useCanvasStore.getState().nodes.find((n) => n.id === ids[2])!;
    useCanvasStore.getState().onNodesChange([
      {
        id: ids[2],
        type: "dimensions",
        resizing: true,
        dimensions: {
          width: ((node.width as number) ?? 0) + 120,
          height: ((node.height as number) ?? 0) + 100,
        },
      },
    ]);
    const layoutAfter = useCanvasStore.getState().tileLayout;
    const tree = layoutAfter?.tree;
    expect(tree?.kind).toBe("row");
    // Left stack still has the first two, right pane still has the third.
    const kids = tree && tree.kind !== "leaf" ? tree.kids : [];
    // Walk leaves of each side.
    const leaves = (n: typeof kids[0] | undefined): string[] => {
      if (!n) return [];
      if (n.kind === "leaf") return [n.id];
      return n.kids.flatMap(leaves);
    };
    expect(leaves(kids[0])).toEqual([ids[0], ids[1]]);
    expect(leaves(kids[1])).toEqual([ids[2]]);
    expectColumnsGapFree();
  });

  it("re-tiling from positions preserves a column arrangement", () => {
    const s = useCanvasStore.getState();
    const ids = [s.addVps(vps(1)), s.addVps(vps(2)), s.addVps(vps(3))];
    useCanvasStore.getState().setPaneSize(PANE);
    useCanvasStore.getState().setTileColumns([2, 1]);
    expect(useCanvasStore.getState().tileLayout?.columns).toBeTruthy();

    // The Tile button path: retileFromPositions re-derives from where nodes sit.
    useCanvasStore.getState().retileFromPositions(PANE);
    const layout = useCanvasStore.getState().tileLayout;
    expect(layout?.columns).toBeTruthy();
    expect(layout?.columns?.length).toBe(2);
    expect(layout?.columns?.[0].items.map((i) => i.id)).toEqual([ids[0], ids[1]]);
    expect(layout?.columns?.[1].items.map((i) => i.id)).toEqual([ids[2]]);
    expectColumnsGapFree();
  });

  it("returns to row layout when the column view is dropped", () => {
    const s = useCanvasStore.getState();
    s.addVps(vps(1));
    s.addVps(vps(2));
    s.addVps(vps(3));
    useCanvasStore.getState().setPaneSize(PANE);
    useCanvasStore.getState().setTileColumns([2, 1]);
    expect(useCanvasStore.getState().tileLayout?.columns).toBeTruthy();

    useCanvasStore.getState().setTileRows([3]);
    expect(useCanvasStore.getState().tileLayout?.columns).toBeUndefined();
    expectGapFree();
  });

  it("re-tiling keeps 1-over-2 on the left beside a full-height right pane", () => {
    const s = useCanvasStore.getState();
    const ids = [s.addVps(vps(1)), s.addVps(vps(2)), s.addVps(vps(3)), s.addVps(vps(4))];
    useCanvasStore.setState({
      layoutMode: "tile",
      paneSize: PANE,
      nodes: useCanvasStore.getState().nodes.map((n) => {
        if (n.id === ids[0]) return { ...n, position: { x: 0, y: 0 }, width: 800, height: 450 };
        if (n.id === ids[1]) return { ...n, position: { x: 0, y: 450 }, width: 400, height: 450 };
        if (n.id === ids[2]) return { ...n, position: { x: 400, y: 450 }, width: 400, height: 450 };
        return { ...n, position: { x: 800, y: 0 }, width: 800, height: 900 };
      }),
    });

    useCanvasStore.getState().retileFromPositions(PANE);

    const byId = new Map(tiles().map((t) => [t.id, t]));
    const top = byId.get(ids[0])!;
    const bottomLeft = byId.get(ids[1])!;
    const bottomRight = byId.get(ids[2])!;
    const right = byId.get(ids[3])!;

    expect(top.y).toBe(0);
    expect(top.x).toBe(0);
    expect(bottomLeft.y).toBe(top.h);
    expect(bottomRight.y).toBe(bottomLeft.y);
    expect(bottomRight.x).toBe(bottomLeft.x + bottomLeft.w);
    expect(right.x).toBe(top.w);
    expect(right.y).toBe(0);
    expect(right.h).toBe(PANE.height);
    expect(bottomLeft.x).not.toBe(bottomRight.x);
  });
});

describe("queued terminal commands (Execute button)", () => {
  beforeEach(() => {
    useCanvasStore.getState().clear();
  });

  it("queues and takes a command once (send=true)", () => {
    const s = useCanvasStore.getState();
    const nodeId = s.addVps(vps(1));
    s.queueTerminalCommand(nodeId, "gh auth login", true);
    const taken = s.takeTerminalCommand(nodeId);
    expect(taken).toEqual({ command: "gh auth login", send: true });
    // Second take is empty (taken once).
    expect(s.takeTerminalCommand(nodeId)).toBeNull();
  });

  it("types without sending when send=false", () => {
    const s = useCanvasStore.getState();
    const nodeId = s.addVps(vps(1));
    s.queueTerminalCommand(nodeId, "echo hi", false);
    expect(s.takeTerminalCommand(nodeId)).toEqual({ command: "echo hi", send: false });
  });
});
