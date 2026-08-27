import { describe, expect, it } from "vitest";
import type { AgentConversationMeta } from "../../../src/lib/tauri";
import { filterAgentConversations, shouldShowHistoryFilter } from "./historyFilter";

function conversation(
  id: string,
  title: string,
  summary?: string | null,
): AgentConversationMeta {
  return { id, title, summary };
}

const chats = [
  conversation("one", "Deploy API", "Review the production rollout"),
  conversation("two", "Database notes", "Investigate the replica lag"),
  conversation("three", "Release checklist", null),
];

describe("shouldShowHistoryFilter", () => {
  it("hides the filter for zero or one conversation", () => {
    expect(shouldShowHistoryFilter([])).toBe(false);
    expect(shouldShowHistoryFilter([chats[0]])).toBe(false);
  });

  it("shows the filter for two or more conversations", () => {
    expect(shouldShowHistoryFilter(chats.slice(0, 2))).toBe(true);
    expect(shouldShowHistoryFilter(chats)).toBe(true);
  });
});

describe("filterAgentConversations", () => {
  it("returns all conversations in order for an empty or whitespace query", () => {
    expect(filterAgentConversations(chats, "")).toBe(chats);
    expect(filterAgentConversations(chats, "  ")).toBe(chats);
  });

  it("trims and matches titles case-insensitively", () => {
    expect(filterAgentConversations(chats, "  API ")).toEqual([chats[0]]);
  });

  it("matches summaries", () => {
    expect(filterAgentConversations(chats, "REPLICA")).toEqual([chats[1]]);
  });

  it("handles null and omitted summaries", () => {
    const withoutSummary = conversation("four", "Untitled chat");
    expect(filterAgentConversations([chats[2], withoutSummary], "summary")).toEqual([]);
  });

  it("returns no conversations when nothing matches", () => {
    expect(filterAgentConversations(chats, "missing")).toEqual([]);
  });

  it("preserves each matching conversation once", () => {
    const both = conversation("both", "API deployment", "Review the API deployment");
    const result = filterAgentConversations([both], "api");
    expect(result).toEqual([both]);
    expect(result).toHaveLength(1);
  });
});

