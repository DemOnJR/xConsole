import type { AgentActivityItem, AgentChatMessage } from "../../stores/agentStore";

export type AgentConsoleRow =
  | { kind: "user"; content: string }
  | { kind: "assistant"; content: string }
  | { kind: "command"; label: string; state: AgentActivityItem["state"] }
  | { kind: "edit"; label: string; added: number; removed: number; state: AgentActivityItem["state"] }
  | { kind: "tool"; label: string; state: AgentActivityItem["state"] };

export function consoleRows(messages: AgentChatMessage[]): AgentConsoleRow[] {
  const rows: AgentConsoleRow[] = [];
  for (const message of messages) {
    if (message.role === "user") {
      rows.push({ kind: "user", content: message.content });
    } else if (message.role === "assistant") {
      rows.push({ kind: "assistant", content: message.content });
      for (const item of message.activity ?? []) {
        const row = activityRow(item);
        if (row) rows.push(row);
      }
    }
  }
  return rows;
}

function activityRow(item: AgentActivityItem): AgentConsoleRow | null {
  if (item.kind === "status" || item.id === "collapsed-meta") return null;
  if (item.kind === "command") {
    return { kind: "command", label: "run command", state: item.state };
  }
  if (item.kind === "file_edit") {
    return {
      kind: "edit",
      label: "edit file",
      added: item.linesAdded ?? 0,
      removed: item.linesRemoved ?? 0,
      state: item.state,
    };
  }
  if (item.kind === "tool" || item.kind === "skill_read" || item.kind === "skill_save") {
    return { kind: "tool", label: item.name ? `tool ${item.name}` : "tool", state: item.state };
  }
  return null;
}
