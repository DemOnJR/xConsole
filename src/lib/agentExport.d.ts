import type { AgentChatMessage } from "../stores/agentStore";
export declare function redactExportText(input: string): string;
export declare function exportConversationMarkdown(input: {
    title?: string | null;
    messages: AgentChatMessage[];
}): string;
//# sourceMappingURL=agentExport.d.ts.map