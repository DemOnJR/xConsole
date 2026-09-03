import { describe, expect, it } from "vitest";
import {
  activitySummary,
  formatDuration,
  isCommandItem,
  liveGerund,
  liveStatusFromActivity,
  visibleActivityItems,
} from "./AgentActivity";
import type { AgentActivityItem } from "../../../src/stores/agentStore";

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

describe("liveGerund", () => {
  it("says executing for a host command", () => {
    expect(
      liveGerund(
        item({
          id: "1",
          kind: "tool",
          label: "Run on K8S",
          tool: "run_command",
          detail: "ufw status verbose",
        }),
      ),
    ).toMatch(/^Executing ufw status/i);
  });

  it("says searching / editing for the cheap file path", () => {
    expect(
      liveGerund(item({ id: "g", kind: "tool", label: "Search cowrie on K8S", tool: "grep_search" })),
    ).toMatch(/^Searching /);
    expect(
      liveGerund(item({ id: "e", kind: "tool", label: "Edit /etc/x.conf on K8S", tool: "edit_file" })),
    ).toMatch(/^Editing /);
  });

  it("says reading / writing for file tools", () => {
    expect(
      liveGerund(item({ id: "r", kind: "tool", label: "Read file · /etc/hosts", tool: "read_file" })),
    ).toMatch(/^Reading /);
    expect(
      liveGerund(item({ id: "w", kind: "file_edit", label: "cowrie.service", path: "/etc/systemd/system/cowrie.service" })),
    ).toMatch(/^Writing /);
  });
});

describe("liveStatusFromActivity", () => {
  it("names the running tool instead of a rotating verb", () => {
    expect(
      liveStatusFromActivity([
        item({
          id: "r",
          kind: "tool",
          label: "Read file · /etc/hosts",
          tool: "read_file",
          state: "running",
        }),
      ]),
    ).toMatch(/^Reading /);
  });

  it("says Thinking when the model has no tool in flight", () => {
    expect(
      liveStatusFromActivity([
        item({ id: "r", kind: "tool", label: "Read /etc/hosts", tool: "read_file", state: "done" }),
      ]),
    ).toBe("Thinking");
  });

  it("counts parallel running tools", () => {
    expect(
      liveStatusFromActivity([
        item({ id: "a", kind: "tool", label: "Read a", tool: "read_file", state: "running" }),
        item({ id: "b", kind: "tool", label: "Read b", tool: "read_file", state: "running" }),
      ]),
    ).toBe("Running 2 tools");
  });
});

describe("activitySummary", () => {
  it("names executed / read / wrote instead of a bare count", () => {
    const text = activitySummary([
      item({ id: "c1", kind: "command", label: "Run on K8S", detail: "uptime" }),
      item({ id: "r", kind: "tool", label: "Read file · /etc/hosts", tool: "read_file" }),
      item({ id: "w", kind: "file_edit", label: "x", path: "/tmp/x" }),
    ]);
    expect(text).toContain("executed 1 command");
    expect(text).toContain("read 1 file");
    expect(text).toContain("wrote 1 file");
  });
});


describe("tool durations", () => {
  it("shows a duration only when it is worth reading", () => {
    // Sub-100ms is noise on a row that is mostly about what ran; "0.0s" is worse than
    // nothing.
    expect(formatDuration(undefined)).toBe("");
    expect(formatDuration(40)).toBe("");
    expect(formatDuration(450)).toBe("450ms");
    expect(formatDuration(2_400)).toBe("2.4s");
    expect(formatDuration(95_000)).toBe("1m 35s");
  });
});
