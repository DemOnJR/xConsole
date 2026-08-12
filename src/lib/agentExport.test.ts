import { describe, expect, it } from "vitest";
import type { AgentChatMessage } from "../stores/agentStore";
import { exportConversationMarkdown, redactExportText } from "./agentExport";

function assistant(content: string, activity?: AgentChatMessage["activity"]): AgentChatMessage {
  return { role: "assistant", content, activity };
}

describe("safe Markdown export", () => {
  it("omits session metadata and raw operational activity details", () => {
    const markdown = exportConversationMarkdown({
      title: "Deploy API",
      messages: [
        { role: "user", content: "Deploy the API." },
        assistant("Deployment complete.", [
          { id: "status-1", kind: "status", label: "Working…", state: "done" },
          {
            id: "cmd-1",
            kind: "command",
            label: "Run on fixture-host",
            detail: "mysql -u root -pTEST_PASSWORD",
            state: "done",
          },
          {
            id: "edit-1",
            kind: "file_edit",
            label: "C:\\secret\\config.env",
            path: "C:\\secret\\config.env",
            linesAdded: 3,
            linesRemoved: 1,
            hunks: [{ kind: "context", text: "TOKEN=TEST_TOKEN" }],
            state: "done",
          },
          { id: "collapsed-meta", kind: "tool", label: "internal", state: "done" },
        ]),
      ],
    });

    expect(markdown).not.toContain("<!-- session");
    expect(markdown).not.toContain("TEST_PASSWORD");
    expect(markdown).not.toContain("TEST_TOKEN");
    expect(markdown).not.toContain("fixture-host");
    expect(markdown).not.toContain("config.env");
    expect(markdown).not.toContain("internal");
    expect(markdown).toContain("- Ran a command");
    expect(markdown).toContain("- Edited a file (+3/-1 lines)");
  });

  it("preserves safe Markdown content and ordering", () => {
    const markdown = exportConversationMarkdown({
      title: null,
      messages: [
        { role: "user", content: "Explain **blue-green deployment**." },
        assistant("```bash\npnpm test\n```"),
      ],
    });
    expect(markdown).toContain("# Conversation");
    expect(markdown).toContain("Explain **blue-green deployment**.");
    expect(markdown).toContain("```bash\npnpm test\n```");
    expect(markdown.indexOf("## User")).toBeLessThan(markdown.indexOf("## Assistant"));
  });

  it("redacts high-confidence secrets without over-redacting safe text", () => {
    const input = "status is healthy; token=TEST_TOKEN; postgres://admin:TEST_PASSWORD@example.invalid/db";
    const output = redactExportText(input);
    expect(output).toContain("status is healthy");
    expect(output).not.toContain("TEST_TOKEN");
    expect(output).not.toContain("TEST_PASSWORD");
    expect(redactExportText(output)).toBe(output);
  });

  it("skips tool-role messages and empty activity", () => {
    const markdown = exportConversationMarkdown({
      title: "Test",
      messages: [
        { role: "tool", content: "hidden tool output" },
        assistant("Visible"),
      ],
    });
    expect(markdown).not.toContain("hidden tool output");
    expect(markdown).toContain("Visible");
    expect(markdown).not.toContain("### Activity");
  });
});
