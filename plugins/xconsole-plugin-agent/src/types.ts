export interface GoalTaskItem {
  id: string;
  text: string;
  done: boolean;
}

export interface AgentChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  created_at?: string;
  tokens?: number;
}
