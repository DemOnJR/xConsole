import { describe, expect, it } from "vitest";
import { activityKind, motionForKind } from "./activityMotion";

describe("activityKind", () => {
  it("maps tools to kinds and kinds to distinct motions", () => {
    expect(activityKind()).toBe("think");
    expect(activityKind({ kind: "tool", tool: "read_file", label: "Read file · /etc/hosts", state: "running" })).toBe("read");
    expect(activityKind({ kind: "file_edit", tool: "", label: "x", state: "running", path: "/tmp/x" })).toBe("write");
    expect(activityKind({ kind: "tool", tool: "run_command", label: "Run on K8S", state: "running" })).toBe("exec");
    expect(activityKind({ kind: "tool", tool: "grep_search", label: "Search", state: "running" })).toBe("search");
    expect(activityKind({ kind: "tool", tool: "edit_file", label: "Edit x", state: "error" })).toBe("error");
  });

  it("gives each kind its own hash motion", () => {
    const motions = [
      "think",
      "read",
      "write",
      "edit",
      "exec",
      "search",
      "todo",
      "connect",
      "work",
      "error",
    ].map((k) => motionForKind(k as Parameters<typeof motionForKind>[0]));
    expect(new Set(motions).size).toBe(10);
  });
});
