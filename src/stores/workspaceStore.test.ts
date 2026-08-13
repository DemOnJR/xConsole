import { describe, expect, it } from "vitest";
import { deserializeTiles, parseSavedNodes } from "./workspaceStore";

describe("workspace tile persistence (columns)", () => {
  it("round-trips a column layout through the saved nodes_json shape", () => {
    // This is exactly what `save()` writes now: tiles = rows (flat mirror), columns
    // = the side-by-side panes. parseSavedNodes is what `restore()` reads.
    const nodesJson = JSON.stringify({
      nodes: [
        { id: "n0", vpsId: "v0", name: "a", host: "h", x: 0, y: 0, width: 800, height: 450 },
        { id: "n1", vpsId: "v1", name: "b", host: "h", x: 0, y: 450, width: 800, height: 450 },
        { id: "n2", vpsId: "v2", name: "c", host: "h", x: 800, y: 0, width: 800, height: 900 },
      ],
      edges: [],
      tiles: [{ weight: 1, items: [{ index: 0, weight: 1 }, { index: 1, weight: 1 }, { index: 2, weight: 1 }] }],
      columns: [
        { weight: 1, items: [{ index: 0, weight: 1 }, { index: 1, weight: 1 }] },
        { weight: 1, items: [{ index: 2, weight: 1 }] },
      ],
    });
    const parsed = parseSavedNodes(nodesJson);
    expect(parsed.columns).toBeDefined();
    expect(parsed.columns!.length).toBe(2);
    expect(parsed.columns![0].items.map((i) => i.index)).toEqual([0, 1]);
    expect(parsed.columns![1].items.map((i) => i.index)).toEqual([2]);
    // The flat row mirror is still there for legacy readers.
    expect(parsed.tiles.length).toBe(1);
  });

  it("degrades gracefully for legacy row-only saves", () => {
    const nodesJson = JSON.stringify({
      nodes: [],
      edges: [],
      tiles: [{ weight: 1, items: [{ index: 0, weight: 1 }] }],
    });
    const parsed = parseSavedNodes(nodesJson);
    expect(parsed.columns).toBeUndefined();
    expect(parsed.tiles.length).toBe(1);
  });

  it("keeps a goal board's session id through the saved nodes_json", () => {
    const parsed = parseSavedNodes(
      JSON.stringify({
        nodes: [
          {
            vpsId: "",
            name: "Goal",
            host: "",
            x: 0,
            y: 0,
            width: 700,
            height: 380,
            nodeType: "goal",
            goalId: "goal-abc",
          },
        ],
        edges: [],
      }),
    );
    expect(parsed.nodes[0]?.nodeType).toBe("goal");
    expect(parsed.nodes[0]?.goalId).toBe("goal-abc");
  });

  it("degrades gracefully for corrupt blobs", () => {
    const parsed = parseSavedNodes("{not json");
    expect(parsed.nodes).toEqual([]);
    expect(parsed.columns).toBeUndefined();
  });

  it("restore keeps a nested tree even when a flat column fallback is also saved", () => {
    const tree = {
      kind: "row" as const,
      weight: 1,
      kids: [
        {
          kind: "col" as const,
          weight: 1,
          kids: [
            { kind: "leaf" as const, index: 0, weight: 1 },
            {
              kind: "row" as const,
              weight: 1,
              kids: [
                { kind: "leaf" as const, index: 1, weight: 1 },
                { kind: "leaf" as const, index: 2, weight: 1 },
              ],
            },
          ],
        },
        { kind: "leaf" as const, index: 3, weight: 1 },
      ],
    };
    const layout = deserializeTiles(
      [{ weight: 1, items: [0, 1, 2, 3].map((index) => ({ index, weight: 1 })) }],
      [
        { weight: 1, items: [0, 1, 2].map((index) => ({ index, weight: 1 })) },
        { weight: 1, items: [{ index: 3, weight: 1 }] },
      ],
      ["a", "b", "c", "d"],
      tree,
    );
    expect(layout?.tree?.kind).toBe("row");
    const left = layout?.tree && layout.tree.kind !== "leaf" ? layout.tree.kids[0] : null;
    expect(left?.kind).toBe("col");
    const bands = left && left.kind === "col" ? left.kids : [];
    expect(bands).toHaveLength(2);
    expect(bands[0].kind === "leaf" && bands[0].id === "a").toBe(true);
    expect(bands[1].kind).toBe("row");
  });
});
