import { describe, expect, it } from "vitest";
import {
  deserializeTiles,
  parseSavedNodes,
  restoredNodeIds,
  workspaceNodeId,
  type SavedNode,
} from "./workspaceStore";

const node = (id: string | undefined, name: string): SavedNode => ({
  id,
  vpsId: "vps-k8s",
  name,
  host: "217.160.69.3",
  x: 0,
  y: 0,
  width: 800,
  height: 450,
  nodeType: "terminal",
});

describe("node identity across a close", () => {
  it("never hands a live node the id of a different one", () => {
    // The bug this exists for: ids used to be derived from the array index, so closing
    // one terminal renamed every terminal after it. The node that had been `ws::2` took
    // over `ws::1` — an id that still belonged to another live SSH session — and the two
    // survivors ended up rendering the same terminal while the third kept running on the
    // server with nothing showing it.
    const before = [node("uuid-a", "a"), node("uuid-b", "b"), node("uuid-c", "c")];
    const idsBefore = restoredNodeIds("ws", before);
    expect(idsBefore).toEqual(["uuid-a", "uuid-b", "uuid-c"]);

    // Close the middle one. The survivors are saved and restored in the new order.
    const after = [before[0], before[2]];
    const idsAfter = restoredNodeIds("ws", after);

    expect(idsAfter).toEqual(["uuid-a", "uuid-c"]);
    // Stated as the invariant rather than as literals, because this is the property
    // that matters: a surviving node's id is its own, not a slot it happens to occupy.
    for (const [i, n] of after.entries()) {
      expect(idsAfter[i]).toBe(idsBefore[before.indexOf(n)]);
    }
  });

  it("falls back to the slot only for saves with no stored id", () => {
    const ids = restoredNodeIds("ws", [node(undefined, "a"), node(undefined, "b")]);
    expect(ids).toEqual([workspaceNodeId("ws", 0), workspaceNodeId("ws", 1)]);
  });

  it("mixes stored and missing ids without collision", () => {
    // A workspace saved before ids were stored, then edited: some nodes have one and
    // some do not. A slot fallback must not land on an id another node already holds.
    const ids = restoredNodeIds("ws", [
      node("ws::1", "a"),
      node(undefined, "b"),
      node("uuid-c", "c"),
    ]);
    expect(new Set(ids).size).toBe(3);
    expect(ids[0]).toBe("ws::1");
    expect(ids[2]).toBe("uuid-c");
  });

  it("refuses to let a duplicated id put two panes on one session", () => {
    // A corrupt or hand-edited save. Two nodes sharing an id is precisely the state
    // that mirrors one terminal into two panes, so the repeat is given its slot.
    const ids = restoredNodeIds("ws", [node("same", "a"), node("same", "b")]);
    expect(ids[0]).toBe("same");
    expect(ids[1]).not.toBe("same");
    expect(new Set(ids).size).toBe(2);
  });
});

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
