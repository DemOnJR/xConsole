import { describe, expect, it } from "vitest";
import type { StreamEvent } from "../lib/tauri";
import {
  appendTextDelta,
  applyActivityEvent,
  flattenActivity,
  segmentsFromMessage,
  textFromSegments,
} from "./turnSegments";

const toolStart = (id: string, label: string): StreamEvent => ({
  kind: "Activity",
  data: { type: "ToolStart", data: { id, tool: "run_command", label, detail: "uptime" } },
});

const toolEnd = (id: string): StreamEvent => ({
  kind: "Activity",
  data: { type: "ToolEnd", data: { id, ok: true } },
});

describe("turnSegments", () => {
  it("interleaves text, tools, then more text", () => {
    let segs = appendTextDelta([], "I'll check both hosts.");
    segs = applyActivityEvent(segs, toolStart("c1", "Run on K8S"));
    segs = applyActivityEvent(segs, toolEnd("c1"));
    segs = appendTextDelta(segs, "Both look healthy.");

    expect(segs.map((s) => s.type)).toEqual(["text", "activity", "text"]);
    expect(textFromSegments(segs)).toBe("I'll check both hosts.\n\nBoth look healthy.");
    expect(flattenActivity(segs)).toHaveLength(1);
    expect(flattenActivity(segs)[0].state).toBe("done");
  });

  it("keeps a second tool burst after a mid-turn reply", () => {
    let segs = applyActivityEvent([], toolStart("a", "Run on K8S"));
    segs = applyActivityEvent(segs, toolEnd("a"));
    segs = appendTextDelta(segs, "Need a firewall next.");
    segs = applyActivityEvent(segs, toolStart("b", "Run on PORTAINER"));
    segs = applyActivityEvent(segs, toolEnd("b"));
    segs = appendTextDelta(segs, "Firewall is live.");

    expect(segs.map((s) => s.type)).toEqual(["activity", "text", "activity", "text"]);
    expect(flattenActivity(segs).map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("falls back to text-then-activity when a message has no segments", () => {
    const segs = segmentsFromMessage({
      role: "assistant",
      content: "Done",
      activity: [{ id: "c1", kind: "command", label: "Run on x", state: "done" }],
    });
    expect(segs.map((s) => s.type)).toEqual(["text", "activity"]);
  });
});
