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
