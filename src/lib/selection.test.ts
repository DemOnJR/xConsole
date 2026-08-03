import { describe, expect, it } from "vitest";
import { actionTargets, parseExtensions, rangeBetween } from "./selection";

const row = (path: string) => ({ path });

describe("actionTargets", () => {
  /// The whole point of a selection: one menu action, six files.
  it("acts on the whole selection when the clicked row is part of it", () => {
    const sel = new Set(["/a", "/b", "/c"]);
    expect(actionTargets(row("/b"), sel).sort()).toEqual(["/a", "/b", "/c"]);
  });

  /// ...and the mirror image, which is the dangerous one. Right-clicking outside the
  /// selection must not delete the selection the user had forgotten about.
  it("acts on one row when that row is outside the selection", () => {
    const sel = new Set(["/a", "/b", "/c"]);
    expect(actionTargets(row("/z"), sel)).toEqual(["/z"]);
  });

  it("falls back to the selection when invoked from empty space", () => {
    expect(actionTargets(null, new Set(["/a", "/b"])).sort()).toEqual(["/a", "/b"]);
  });

  it("does nothing when there is nothing to act on", () => {
    expect(actionTargets(null, new Set())).toEqual([]);
  });
});

describe("rangeBetween", () => {
  const list = ["/1", "/2", "/3", "/4", "/5"];

  it("covers both ends, in either direction", () => {
    expect(rangeBetween(list, "/2", "/4")).toEqual(["/2", "/3", "/4"]);
    expect(rangeBetween(list, "/4", "/2")).toEqual(["/2", "/3", "/4"]);
  });

  it("is a single row when the anchor is the row", () => {
    expect(rangeBetween(list, "/3", "/3")).toEqual(["/3"]);
  });

  /// The listing gets filtered and refreshed under the anchor all the time. A stale
  /// anchor must degrade to "just this row", never to an empty or arbitrary range.
  it("survives an anchor that is no longer on screen", () => {
    expect(rangeBetween(list, "/gone", "/3")).toEqual(["/3"]);
    expect(rangeBetween(list, null, "/3")).toEqual(["/3"]);
  });

  it("returns nothing when the clicked row is not on screen either", () => {
    expect(rangeBetween(list, "/2", "/gone")).toEqual([]);
  });
});

describe("parseExtensions", () => {
  it("accepts every way people type a list", () => {
    expect(parseExtensions("php js")).toEqual(["php", "js"]);
    expect(parseExtensions("php,js")).toEqual(["php", "js"]);
    expect(parseExtensions("php, js,  ts")).toEqual(["php", "js", "ts"]);
  });

  /// A leading dot is how an extension is written and never how it is stored.
  it("strips leading dots", () => {
    expect(parseExtensions(".php, .JS")).toEqual(["php", "js"]);
    expect(parseExtensions("...php")).toEqual(["php"]);
  });

  it("yields nothing for nothing", () => {
    expect(parseExtensions("")).toEqual([]);
    expect(parseExtensions("   ,, . ")).toEqual([]);
  });
});
