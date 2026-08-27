import { describe, expect, it } from "vitest";
import { parseChecklist, findLatestChecklist } from "./StickyChecklist";
import { formatWorkingDuration, type AgentChatMessage } from "../../../src/stores/agentStore";

describe("StickyChecklist", () => {
  it("parses todo checklist items properly", () => {
    const raw = `[x] Step 1: Create API endpoint
[>] Step 2: Write tests in sso.rs
[ ] Step 3: Configure docker compose`;

    const items = parseChecklist(raw);
    expect(items.length).toBe(3);
    expect(items[0].status).toBe("done");
    expect(items[0].text).toBe("Step 1: Create API endpoint");

    expect(items[1].status).toBe("active");
    expect(items[1].text).toBe("Step 2: Write tests in sso.rs");

    expect(items[2].status).toBe("pending");
    expect(items[2].text).toBe("Step 3: Configure docker compose");
  });

  it("identifies fully completed 10/10 checklists", () => {
    const raw = `[x] Item 1\n[x] Item 2\n[x] Item 3`;
    const items = parseChecklist(raw);
    const doneCount = items.filter((x) => x.status === "done").length;
    expect(doneCount).toBe(items.length);
    expect(items.length).toBe(3);
  });

  it("formats working duration strings properly across ranges", () => {
    expect(formatWorkingDuration(12000)).toBe("Worked for 12s");
    expect(formatWorkingDuration(135000)).toBe("Worked for 2m 15s");
    expect(formatWorkingDuration(3724000)).toBe("Worked for 1h 2m 4s");
  });

  it("finds latest checklist from messages or activity", () => {
    const messages: AgentChatMessage[] = [
      {
        role: "assistant",
        content: "Starting task",
        activity: [
          {
            id: "act-1",
            kind: "tool",
            tool: "todo_write",
            label: "Update checklist",
            output: "[>] First step\n[ ] Second step",
            state: "done",
          },
        ],
      },
    ];

    const found = findLatestChecklist(messages, [], []);
    expect(found).toBe("[>] First step\n[ ] Second step");
  });
});


