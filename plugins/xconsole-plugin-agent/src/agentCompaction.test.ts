import { describe, expect, it } from "vitest";
import type { AgentChatMessage } from "../../../src/stores/agentStore";
import { exportConversationMarkdown } from "../../../src/lib/agentExport";
import { consoleRows } from "./agentConsoleRows";

describe("Agent Compaction Transcript Preservation", () => {
  it("preserves full transcript history across multiple turns with compaction", () => {
    const transcript: AgentChatMessage[] = [
      { role: "user", content: "Initial server audit" },
      {
        role: "assistant",
        content: "Here is the audit report.",
        activity: [
          {
            id: "cmd-1",
            kind: "command",
            label: "run command",
            detail: "uname -a",
            state: "done",
          },
        ],
      },
      // Compaction event occurred after turn 1
      {
        role: "system",
        content: "Context compacted",
        isCompaction: true,
        compactionTokensBefore: 35000,
        compactionTokensAfter: 4800,
        compactionPrunedTools: 6,
      },
      { role: "user", content: "Now patch the packages" },
      {
        role: "assistant",
        content: "Packages patched successfully.",
        activity: [
          {
            id: "edit-1",
            kind: "file_edit",
            label: "edit file",
            linesAdded: 3,
            linesRemoved: 1,
            state: "done",
          },
        ],
      },
    ];

    const rows = consoleRows(transcript);
    expect(rows.map((r) => r.kind)).toEqual([
      "user",
      "assistant",
      "command",
      "compaction",
      "user",
      "assistant",
      "edit",
    ]);

    // Ensure all user turns and assistant responses remain in the transcript
    expect(rows[0]).toEqual({ kind: "user", content: "Initial server audit" });
    expect(rows[1]).toEqual({ kind: "assistant", content: "Here is the audit report." });
    expect(rows[3]).toMatchObject({
      kind: "compaction",
      tokensBefore: 35000,
      tokensAfter: 4800,
      prunedTools: 6,
    });
    expect(rows[4]).toEqual({ kind: "user", content: "Now patch the packages" });
    expect(rows[5]).toEqual({ kind: "assistant", content: "Packages patched successfully." });

    // Export test
    const exported = exportConversationMarkdown({
      title: "Cluster maintenance",
      messages: transcript,
    });

    expect(exported).toContain("Initial server audit");
    expect(exported).toContain("Here is the audit report.");
    expect(exported).toContain("⚡ Context compacted (~35000 → ~4800 tokens)");
    expect(exported).toContain("Now patch the packages");
    expect(exported).toContain("Packages patched successfully.");
  });
});
