import { describe, expect, it } from "vitest";
import { stableViewport, workspacePersistKey, type PersistableCanvas } from "./workspacePersist";

function canvas(over: Partial<PersistableCanvas> = {}): PersistableCanvas {
  return {
    nodes: [
      {
        id: "ws::0",
        type: "terminal",
        position: { x: 10.2, y: 20.8 },
        width: 460.4,
        height: 320.1,
        data: { vpsId: "v1", name: "box", host: "1.2.3.4" },
      },
    ],
    edges: [],
    layoutMode: "freeform",
    tileLayout: null,
    ...over,
  };
}

const vp = { x: 1.234, y: 5.678, zoom: 1.00001 };

describe("workspacePersistKey", () => {
  it("ignores selection, dragging, and subpixel position noise", () => {
    const a = workspacePersistKey(canvas(), vp);
    const b = workspacePersistKey(
      canvas({
        nodes: [
          {
            id: "ws::0",
            type: "terminal",
            position: { x: 10.4, y: 20.6 },
            width: 460.1,
            height: 320.4,
            data: {
              vpsId: "v1",
              name: "box",
              host: "1.2.3.4",
              selected: true,
              dragging: true,
            },
          },
        ],
      }),
      { x: 1.231, y: 5.682, zoom: 1.00004 },
    );
    expect(a).toBe(b);
  });

  it("changes when a node actually moves or the layout changes", () => {
    const a = workspacePersistKey(canvas(), vp);
    const moved = workspacePersistKey(
      canvas({
        nodes: [
          {
            id: "ws::0",
            type: "terminal",
            position: { x: 80, y: 20 },
            width: 460,
            height: 320,
            data: { vpsId: "v1", name: "box", host: "1.2.3.4" },
          },
        ],
      }),
      vp,
    );
    const tiled = workspacePersistKey(canvas({ layoutMode: "tile" }), vp);
    expect(moved).not.toBe(a);
    expect(tiled).not.toBe(a);
  });
});

describe("stableViewport", () => {
  it("collapses floating-point jitter", () => {
    expect(stableViewport({ x: 1.2344, y: 5.6784, zoom: 1.000014 })).toEqual(
      stableViewport({ x: 1.2341, y: 5.6782, zoom: 1.000009 }),
    );
  });
});
