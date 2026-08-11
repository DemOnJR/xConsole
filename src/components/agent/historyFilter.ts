import type { AgentConversationMeta } from "../../lib/tauri";

export function shouldShowHistoryFilter(conversations: AgentConversationMeta[]): boolean {
  return conversations.length > 1;
}

export function filterAgentConversations(
  conversations: AgentConversationMeta[],
  query: string,
): AgentConversationMeta[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return conversations;

  return conversations.filter(
    (conversation) =>
      conversation.title.toLowerCase().includes(normalizedQuery) ||
      (conversation.summary ?? "").toLowerCase().includes(normalizedQuery),
  );
}
