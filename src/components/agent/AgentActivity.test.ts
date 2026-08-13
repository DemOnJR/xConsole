import { describe, expect, it } from "vitest";
import { isCommandItem, visibleActivityItems } from "./AgentActivity";
import type { AgentActivityItem } from "../../stores/agentStore";

const item = (partial: Partial<AgentActivityItem> & Pick<AgentActivityItem, "id" | "kind" | "label">): AgentActivityItem => ({
  state: "done",
  ...partial,
});

describe("visibleActivityItems", () => {
  it("hides cache hit/miss lines (those belong on the input bar)", () => {
    const visible = visibleActivityItems([
      item({ id: "cache-line", kind: "status", label: "cache 15104 hit · 2736 miss · 85%" }),
      item({ id: "cache-miss", kind: "status", label: "cache miss: 2736 miss · 85% hit — large uncached tail" }),
      item({ id: "c1", kind: "command", label: "Run on K8S", detail: "uptime" }),
      item({ id: "parallel-batch", kind: "status", label: "Running 2 tools in parallel" }),
    ]);
    expect(visible.map((v) => v.id)).toEqual(["c1", "parallel-batch"]);
  });
});

describe("isCommandItem", () => {
  it("treats Run on HOST labels as commands", () => {
    expect(isCommandItem(item({ id: "1", kind: "tool", label: "Run on PORTAINER", tool: "run_command" }))).toBe(true);
  });
});
