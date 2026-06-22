import { create } from "zustand";
import {
  api,
  onAgentApproval,
  onAiChatOutput,
  type AgentApproval,
  type AgentConversationMeta,
  type ChatMessage,
  type StreamEvent,
} from "../lib/tauri";
import {
  liveTokenStats,
  type ContextUsage,
  type TokenStats,
} from "../lib/streamStats";
import type { UnlistenFn } from "@tauri-apps/api/event";

export interface AgentActivityItem {
  id: string;
  kind: "status" | "tool" | "skill_read" | "skill_save" | "command" | "tool_end" | "file_edit";
  label: string;
  detail?: string;
  output?: string;
  state: "running" | "done" | "error";
  category?: string;
  name?: string;
  tool?: string;
  path?: string;
  linesAdded?: number;
  linesRemoved?: number;
  hunks?: { kind: string; text: string }[];
}

/** Token throughput for a completed or streaming assistant message. */
export type { TokenStats, ContextUsage } from "../lib/streamStats";

export interface AgentChatMessage extends ChatMessage {
  activity?: AgentActivityItem[];
  tokenStats?: TokenStats;
}

interface AgentState {
  sessionId: string;
  messages: AgentChatMessage[];
  conversations: AgentConversationMeta[];
  streamingText: string;
  activity: AgentActivityItem[];
  streaming: boolean;
  streamStats: TokenStats | null;
  contextUsage: ContextUsage | null;
  /** Increments when conversation is auto-compacted — drives hourglass flip. */
  compactFlipCount: number;
  error: string | null;
  targets: string[];
  pendingApprovals: AgentApproval[];
  hydrated: boolean;

  init: () => Promise<void>;
  setTargets: (ids: string[]) => void;
  send: (text: string) => Promise<void>;
  newConversation: () => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  removeConversation: (id: string) => Promise<void>;
  subscribeApprovals: () => Promise<UnlistenFn>;
  resolveApproval: (id: string, approved: boolean) => Promise<void>;
}

const newSessionId = () =>
  (crypto.randomUUID && crypto.randomUUID()) ||
  Math.random().toString(36).slice(2);

function applyStreamEvent(
  activity: AgentActivityItem[],
  ev: StreamEvent,
): AgentActivityItem[] {
  switch (ev.kind) {
    case "Status":
      // Internal progress only — not shown in the Cursor-style tool feed.
      return activity;
    case "ToolCall":
      if (activity.some((a) => a.id === ev.data.id)) return activity;
      if (/mcp/i.test(ev.data.name)) return activity;
      return [
        ...activity,
        {
          id: ev.data.id,
          kind: "tool",
          label: ev.data.name.replace(/_/g, " "),
          state: "running",
        },
      ];
    case "ToolResult": {
      if (ev.data.id.startsWith("snapshot-")) return activity;
      const idx = activity.findIndex((a) => a.id === ev.data.id);
      if (idx >= 0) {
        const next = [...activity];
        next[idx] = {
          ...next[idx],
          output: ev.data.output,
          state: ev.data.output.startsWith("error") ? "error" : "done",
        };
        return next;
      }
      return activity;
    }
    case "Activity": {
      const d = ev.data;
      switch (d.type) {
        case "ToolStart":
          return [
            ...activity.filter((a) => !(a.id === d.data.id && a.kind === "tool")),
            {
              id: d.data.id,
              kind: "tool",
              tool: d.data.tool,
              label: d.data.label,
              detail: d.data.detail,
              state: "running",
            },
          ];
        case "FileEdit":
          return [
            ...activity.filter((a) => a.id !== d.data.id),
            {
              id: d.data.id,
              kind: "file_edit",
              label: d.data.path,
              path: d.data.path,
              linesAdded: d.data.lines_added,
              linesRemoved: d.data.lines_removed,
              hunks: d.data.hunks,
              state: "done",
            },
          ];
        case "ToolEnd":
          return activity.map((a) => {
            if (a.id !== d.data.id && !a.id.startsWith(`${d.data.id}-`)) return a;
            if (a.kind === "file_edit") {
              return { ...a, state: d.data.ok ? "done" : "error" };
            }
            if (
              a.kind === "tool" &&
              a.label.startsWith("Write file ·") &&
              a.detail &&
              !activity.some((x) => x.id === a.id && x.kind === "file_edit")
            ) {
              const fullPath = a.label.slice("Write file ·".length).trim();
              const fileName = fullPath.split(/[/\\]/).pop() || fullPath;
              const hunks = a.detail.split("\n").slice(0, 28).map((text) => ({
                kind: "add" as const,
                text,
              }));
              return {
                id: a.id,
                kind: "file_edit" as const,
                label: fileName,
                path: fileName,
                linesAdded: a.detail.split("\n").length,
                linesRemoved: 0,
                hunks,
                state: d.data.ok ? ("done" as const) : ("error" as const),
              };
            }
            if (a.kind === "tool" || a.kind === "skill_read" || a.kind === "command") {
              return { ...a, state: d.data.ok ? "done" : "error" };
            }
            return a;
          });
        case "SkillRead":
          return [
            ...activity,
            {
              id: `${d.data.id}-skill-read`,
              kind: "skill_read",
              label: `Read skill ${d.data.category}/${d.data.name}`,
              category: d.data.category,
              name: d.data.name,
              state: "running",
            },
          ];
        case "SkillSaved":
          return [
            ...activity,
            {
              id: `${d.data.id}-skill-save`,
              kind: "skill_save",
              label: `Saved skill ${d.data.category}/${d.data.name}`,
              category: d.data.category,
              name: d.data.name,
              state: "done",
            },
          ];
        case "Command": {
          const idx = activity.findIndex((a) => a.id === d.data.id);
          if (idx >= 0) {
            const next = [...activity];
            next[idx] = {
              ...next[idx],
              kind: "command",
              label: `Run on ${d.data.vps}`,
              detail: d.data.command,
            };
            return next;
          }
          return [
            ...activity,
            {
              id: d.data.id,
              kind: "command",
              label: `Run on ${d.data.vps}`,
              detail: d.data.command,
              state: "running",
            },
          ];
        }
        default:
          return activity;
      }
    }
    default:
      return activity;
  }
}

async function persistConversation(state: {
  sessionId: string;
  messages: AgentChatMessage[];
  targets: string[];
}) {
  if (state.messages.length === 0) return;
  await api.saveAgentConversation({
    id: state.sessionId,
    targets: state.targets,
    messagesJson: JSON.stringify(state.messages),
  });
}

export const useAgentStore = create<AgentState>((set, get) => ({
  sessionId: newSessionId(),
  messages: [],
  conversations: [],
  streamingText: "",
  activity: [],
  streaming: false,
  streamStats: null,
  contextUsage: null,
  compactFlipCount: 0,
  error: null,
  targets: [],
  pendingApprovals: [],
  hydrated: false,

  init: async () => {
    if (get().hydrated) return;
    try {
      const list = await api.listAgentConversations();
      set({ conversations: list });
      const lastId = await api.getSetting("agent.last_conversation");
      const openId =
        lastId && list.some((c) => c.id === lastId) ? lastId : list[0]?.id;
      if (openId) {
        await get().openConversation(openId);
      }
    } catch {
      // fresh install — start empty
    }
    set({ hydrated: true });
  },

  setTargets: (ids) => set({ targets: ids }),

  subscribeApprovals: () =>
    onAgentApproval((approval) =>
      set((s) =>
        s.pendingApprovals.some((a) => a.id === approval.id)
          ? s
          : { pendingApprovals: [...s.pendingApprovals, approval] },
      ),
    ),

  resolveApproval: async (id, approved) => {
    set((s) => ({
      pendingApprovals: s.pendingApprovals.filter((a) => a.id !== id),
    }));
    await api.agentResolveApproval(id, approved);
  },

  newConversation: async () => {
    const id = newSessionId();
    set({
      sessionId: id,
      messages: [],
      streamingText: "",
      activity: [],
      streaming: false,
      streamStats: null,
      contextUsage: null,
      compactFlipCount: 0,
      error: null,
    });
    const list = await api.listAgentConversations().catch(() => get().conversations);
    set({ conversations: list });
  },

  openConversation: async (id) => {
    const conv = await api.getAgentConversation(id);
    if (!conv) return;
    let messages: AgentChatMessage[] = [];
    try {
      messages = JSON.parse(conv.messages_json) as AgentChatMessage[];
    } catch {
      messages = [];
    }
    let targets: string[] = [];
    if (conv.targets_json) {
      try {
        targets = JSON.parse(conv.targets_json) as string[];
      } catch {
        targets = [];
      }
    }
    set({
      sessionId: id,
      messages,
      targets,
      streamingText: "",
      activity: [],
      streaming: false,
      streamStats: null,
      contextUsage: null,
      compactFlipCount: 0,
      error: null,
    });
    const list = await api.listAgentConversations().catch(() => get().conversations);
    set({ conversations: list });
  },

  removeConversation: async (id) => {
    await api.deleteAgentConversation(id);
    const list = await api.listAgentConversations();
    set({ conversations: list });
    if (get().sessionId === id) {
      if (list.length > 0) {
        await get().openConversation(list[0].id);
      } else {
        await get().newConversation();
      }
    }
  },

  send: async (text) => {
    const trimmed = text.trim();
    if (!trimmed || get().streaming) return;

    const userMsg: AgentChatMessage = { role: "user", content: trimmed };
    const history = [...get().messages, userMsg];
    set({
      messages: history,
      streaming: true,
      streamingText: "",
      activity: [],
      streamStats: null,
      error: null,
    });

    let streamStartedAt: number | null = null;
    let latestStats: TokenStats | null = null;

    const { sessionId, targets } = get();
    const unlisten = await onAiChatOutput(sessionId, (ev) => {
      if (ev.kind === "Text") {
        set((s) => {
          const streamingText = s.streamingText + ev.data;
          if (streamStartedAt === null) {
            streamStartedAt = Date.now();
          }
          const streamStats = liveTokenStats(streamingText, streamStartedAt);
          return { streamingText, streamStats };
        });
        return;
      }
      if (ev.kind === "Stats") {
        latestStats = {
          completionTokens: ev.data.completion_tokens,
          promptTokens: ev.data.prompt_tokens ?? undefined,
          tokensPerSec: ev.data.tokens_per_sec,
          source: "provider",
        };
        set({ streamStats: latestStats });
        return;
      }
      if (ev.kind === "ContextUsage") {
        set({ contextUsage: ev.data });
        return;
      }
      if (ev.kind === "ConversationCompacted") {
        set((s) => ({
          messages: ev.data.map((m) => ({
            role: m.role as "user" | "assistant" | "system" | "tool",
            content: m.content,
          })),
          compactFlipCount: s.compactFlipCount + 1,
        }));
        return;
      }
      if (ev.kind === "Error") {
        set({ error: ev.data });
        return;
      }
      set((s) => ({ activity: applyStreamEvent(s.activity, ev) }));
    });

    try {
      const assistant = await api.aiChat({
        sessionId,
        messages: history,
        targets,
      });
      const activity = get().activity;
      const tokenStats =
        latestStats ??
        (get().streamingText && streamStartedAt
          ? liveTokenStats(get().streamingText, streamStartedAt)
          : undefined);
      const messages = [
        ...get().messages,
        {
          ...assistant,
          activity: activity.length > 0 ? [...activity] : undefined,
          tokenStats,
        },
      ];
      set({
        messages,
        streamingText: "",
        activity: [],
        streaming: false,
        streamStats: null,
      });
      await persistConversation({ sessionId, messages, targets });
      const list = await api.listAgentConversations().catch(() => get().conversations);
      set({ conversations: list });
    } catch (e) {
      const activity = get().activity;
      const messages = get().streamingText
        ? [
            ...get().messages,
            {
              role: "assistant" as const,
              content: get().streamingText,
              activity: activity.length > 0 ? [...activity] : undefined,
            },
          ]
        : get().messages;
      set({
        streaming: false,
        error: String(e),
        messages,
        streamingText: "",
        activity: [],
        streamStats: null,
      });
      if (messages.length > 0) {
        await persistConversation({ sessionId, messages, targets }).catch(() => {});
      }
    } finally {
      unlisten();
    }
  },
}));
