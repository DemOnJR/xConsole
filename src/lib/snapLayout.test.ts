import { describe, expect, it } from "vitest";
import { snapLayout, snapZones, zoneAt } from "./snapLayout";

describe("snapZones", () => {
  it("offers left/right/top/bottom for two nodes", () => {
    const zones = snapZones(2);
    const ids = zones.map((z) => z.id);
    expect(ids).toContain("left");
    expect(ids).toContain("right");
    expect(ids).toContain("top");
    expect(ids).toContain("bottom");
    // No 2-1 sidebars for only 2 nodes.
    expect(ids).not.toContain("side-left");
  });

  it("adds sidebar zones once there are at least three nodes", () => {
    const zones = snapZones(3);
    expect(zones.some((z) => z.id === "side-left")).toBe(true);
    expect(zones.some((z) => z.id === "side-right")).toBe(true);
  });

  it("only offers zones whose shape sums to the node count", () => {
    for (const n of [1, 2, 3, 4, 5, 6]) {
      for (const zone of snapZones(n)) {
        expect(zone.shape.reduce((a, b) => a + b, 0)).toBe(n);
      }
    }
  });
});

describe("zoneAt", () => {
  it("finds the zone under a point, with corners winning over edges", () => {
    const zones = snapZones(3);
    // Corners (listed first) win over the edges that contain them.
    expect(zoneAt(zones, 0.05, 0.05)?.id).toBe("tl");
    expect(zoneAt(zones, 0.95, 0.05)?.id).toBe("tr");
    expect(zoneAt(zones, 0.05, 0.95)?.id).toBe("bl");
    expect(zoneAt(zones, 0.95, 0.95)?.id).toBe("br");
    // Mid-edge points hit the edge zones.
    expect(zoneAt(zones, 0.05, 0.5)?.id).toBe("left");
    expect(zoneAt(zones, 0.5, 0.05)?.id).toBe("top");
    expect(zoneAt(zones, 0.5, 0.95)?.id).toBe("bottom");
    // The middle of the pane is the sidebar zone.
    expect(zoneAt(zones, 0.5, 0.5)?.id).toBe("side-left");
  });
});

describe("snapLayout", () => {
  it("places the dragged node in the left column of a 2-1 sidebar", () => {
    const layout = snapLayout("drag", ["b", "c"], {
      id: "left",
      x: 0,
      y: 0,
      w: 0.5,
      h: 1,
      shape: [1, 2],
      mode: "columns",
      slot: 0,
    });
    expect(layout.columns).toHaveLength(2);
    expect(layout.columns![0].items.map((i) => i.id)).toEqual(["drag"]);
    expect(layout.columns![1].items.map((i) => i.id)).toEqual(["b", "c"]);
  });

  it("places the dragged node in the right column of a 2-1 sidebar", () => {
    const layout = snapLayout("drag", ["a", "b"], {
      id: "right",
      x: 0.5,
      y: 0,
      w: 0.5,
      h: 1,
      shape: [2, 1],
      mode: "columns",
      slot: 1,
    });
    expect(layout.columns![0].items.map((i) => i.id)).toEqual(["a", "b"]);
    expect(layout.columns![1].items.map((i) => i.id)).toEqual(["drag"]);
  });

  it("places the dragged node in the top row of a 1-2 rows layout", () => {
    const layout = snapLayout("drag", ["b", "c"], {
      id: "top",
      x: 0,
      y: 0,
      w: 1,
      h: 0.5,
      shape: [1, 2],
      mode: "rows",
      slot: 0,
    });
    expect(layout.rows).toHaveLength(2);
    expect(layout.rows[0].items.map((i) => i.id)).toEqual(["drag"]);
    expect(layout.rows[1].items.map((i) => i.id)).toEqual(["b", "c"]);
  });

  it("keeps every node exactly once", () => {
    const ids = ["drag", "b", "c", "d"];
    for (const zone of snapZones(ids.length)) {
      const layout = snapLayout("drag", ["b", "c", "d"], zone);
      const placed = layout.columns
        ? layout.columns.flatMap((c) => c.items.map((i) => i.id))
        : layout.rows.flatMap((r) => r.items.map((i) => i.id));
      expect([...placed].sort()).toEqual([...ids].sort());
    }
  });
});
