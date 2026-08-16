import { describe, expect, it } from "vitest";
import { computePlanDiff } from "./planDiff";

describe("computePlanDiff", () => {
  it("handles identical texts with no changes", () => {
    const text = "Line 1\nLine 2\nLine 3";
    const res = computePlanDiff(text, text);
    expect(res.hasChanges).toBe(false);
    expect(res.addedCount).toBe(0);
    expect(res.removedCount).toBe(0);
    expect(res.lines.every((l) => l.kind === "same")).toBe(true);
    expect(res.lines.length).toBe(3);
  });

  it("handles additions and deletions", () => {
    const oldText = "Step 1: Init\nStep 2: Old login\nStep 3: Finish";
    const newText = "Step 1: Init\nStep 2: SSO via OAuth\nStep 2b: Rollback plan\nStep 3: Finish";
    const res = computePlanDiff(oldText, newText);

    expect(res.hasChanges).toBe(true);
    expect(res.removedCount).toBe(1); // "Step 2: Old login"
    expect(res.addedCount).toBe(2); // "Step 2: SSO via OAuth", "Step 2b: Rollback plan"

    const del = res.lines.find((l) => l.kind === "del");
    expect(del?.text).toBe("Step 2: Old login");

    const adds = res.lines.filter((l) => l.kind === "add");
    expect(adds.map((a) => a.text)).toEqual(["Step 2: SSO via OAuth", "Step 2b: Rollback plan"]);
  });

  it("handles empty texts gracefully", () => {
    const res = computePlanDiff("", "");
    expect(res.hasChanges).toBe(false);
    expect(res.lines).toEqual([]);

    const res2 = computePlanDiff("", "New Line");
    expect(res2.addedCount).toBe(1);
    expect(res2.removedCount).toBe(0);
  });
});
