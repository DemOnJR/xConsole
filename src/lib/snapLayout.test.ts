import { describe, expect, it } from "vitest";
import { dropTargetAt } from "./snapLayout";

describe("drop targeting", () => {
  it("docks to the pane edge, not a preset shape", () => {
    const boxes = [{ id: "a", x: 200, y: 100, width: 1200, height: 700 }];
    const left = dropTargetAt(boxes, 10, 450, 1600, 900, "a");
    expect(left?.kind).toBe("pane");
    expect(left?.edge).toBe("left");
  });
});
