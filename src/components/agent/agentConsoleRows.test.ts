import { describe, expect, it } from "vitest";
import type { AgentChatMessage } from "../../stores/agentStore";
import { consoleRows } from "./agentConsoleRows";

function assistant(activity: AgentChatMessage["activity"]): AgentChatMessage {
  return {
    role: "assistant",
    content: "Done",
    activity,
  };
}

describe("Agent Console rows", () => {
  it("preserves message order and converts activity to safe generic rows", () => {
    const rows = consoleRows([
      { role: "user", content: "Deploy" },
      assistant([
        {
          id: "command-1",
          kind: "command",
          label: "Run on fixture-host",
          detail: "mysql -pTEST_PASSWORD",
          state: "done",
        },
        {
          id: "edit-1",
          kind: "file_edit",
          label: "C:/secret/config.env",
          path: "C:/secret/config.env",
          linesAdded: 2,
          linesRemoved: 1,
          hunks: [{ kind: "add", text: "TOKEN=TEST_TOKEN" }],
          state: "done",
        },
        { id: "status-1", kind: "status", label: "private status", state: "done" },
        { id: "collapsed-meta", kind: "tool", label: "internal", state: "done" },
      ]),
    ]);

    expect(rows.map((row) => row.kind)).toEqual(["user", "assistant", "command", "edit"]);
    expect(rows[2]).toMatchObject({ kind: "command", label: "run command" });
    expect(rows[3]).toMatchObject({ kind: "edit", label: "edit file", added: 2, removed: 1 });
    expect(JSON.stringify(rows)).not.toContain("TEST_PASSWORD");
    expect(JSON.stringify(rows)).not.toContain("TEST_TOKEN");
    expect(JSON.stringify(rows)).not.toContain("fixture-host");
    expect(JSON.stringify(rows)).not.toContain("config.env");
  });

  it("keeps ordinary assistant content unchanged", () => {
    const rows = consoleRows([assistant([])]);
    expect(rows).toEqual([{ kind: "assistant", content: "Done" }]);
  });

  it("walks interleaved segments in chronological order", () => {
    const rows = consoleRows([
      { role: "user", content: "check hosts" },
      {
        role: "assistant",
        content: "I'll check.\n\nHealthy.",
        segments: [
          { type: "text", content: "I'll check." },
          {
            type: "activity",
            items: [
              {
                id: "c1",
                kind: "command",
                label: "Run on fixture-host",
                detail: "uptime",
                state: "done",
              },
            ],
          },
          { type: "text", content: "Healthy." },
        ],
      },
    ]);
    expect(rows.map((r) => r.kind)).toEqual(["user", "assistant", "command", "assistant"]);
    expect(rows[1]).toMatchObject({ kind: "assistant", content: "I'll check." });
    expect(rows[3]).toMatchObject({ kind: "assistant", content: "Healthy." });
  });

  it("renders compaction divider row with token reduction metrics", () => {
    const rows = consoleRows([
      { role: "user", content: "Inspect cluster" },
      {
        role: "system",
        content: "Context compacted",
        isCompaction: true,
        compactionTokensBefore: 45000,
        compactionTokensAfter: 6200,
        compactionPrunedTools: 14,
      },
      assistant([]),
    ]);

    expect(rows.map((r) => r.kind)).toEqual(["user", "compaction", "assistant"]);
    expect(rows[1]).toMatchObject({
      kind: "compaction",
      label: "Context compacted",
      tokensBefore: 45000,
      tokensAfter: 6200,
      prunedTools: 14,
    });
  });
});
