import { create } from "zustand";
import {
  api,
  onAgentApproval,
  onAgentPlan,
  onAgentQuestion,
  onAiChatOutput,
  type AgentApproval,
  type AgentConversationMeta,
  type AgentPlan,
  type AgentQuestion,
  type CanvasSnapshotNode,
  type ChatMessage,
  type StreamEvent,
} from "../lib/tauri";
import { notify } from "../lib/notify";
import { useWorkspaceStore } from "./workspaceStore";
import { useCanvasStore } from "./canvasStore";
import { useSessionStore } from "./sessionStore";
import { useVoiceStore } from "./voiceStore";
import {
  cancelSpeech,
  currentSpeechEpoch,
  enqueueSpeechBytes,
  speak,
  speakBytes,
  speakableText,
} from "../lib/voice";

/** Speak text via TTS if the user enabled spoken replies. Uses the natural cloud
 *  voice when selected, falling back to the OS voice if it fails or has no key. */
async function maybeSpeak(raw: string) {
  const v = useVoiceStore.getState();
  const text = speakableText(raw);
  if (!v.ttsEnabled || !text.trim()) return;
  // Track speaking so the Stop button can hush the agent mid-sentence; the lib
  // calls `done` on natural end AND on cancel/stop, so the flag never sticks.
  // Set the flag AFTER playback starts (speakBytes first stops any prior clip,
  // which fires the previous `done`) so overlapping replies don't clear it.
  const setSpeaking = useAgentStore.getState().setSpeaking;
  const done = () => setSpeaking(false);
  if (v.ttsEngine === "piper") {
    try {
      const b64 = await api.synthesize(text, v.ttsPiperVoice || "en_US-hfc_female-medium", "piper");
      speakBytes(b64, done);
      setSpeaking(true);
      return;
    } catch {
      /* fall through to the OS voice */
    }
  } else if (v.ttsEngine === "edge") {
    try {
      const b64 = await api.synthesize(text, v.ttsEdgeVoice || "en-US-AriaNeural", "edge");
      speakBytes(b64, done, "audio/mpeg");
      setSpeaking(true);
      return;
    } catch {
      /* fall through to the OS voice */
    }
  } else if (v.ttsEngine === "cloud") {
    try {
      const b64 = await api.synthesize(text, v.ttsCloudVoice || "sage", "cloud", v.ttsInstructions);
      speakBytes(b64, done);
      setSpeaking(true);
      return;
    } catch {
      /* fall through to the OS voice */
    }
  }
  speak(text, { voice: v.ttsVoice || undefined, rate: v.ttsRate, onEnd: done });
  setSpeaking(true);
}

/** Pull complete sentences off a growing buffer for streaming TTS. Splits on . ! ?
 *  or newline, but only when the segment is long enough, so abbreviations ("e.g.",
 *  "v1.") don't trigger a split. Returns finished sentences + the unfinished tail. */
function extractSentences(buf: string): { sentences: string[]; rest: string } {
  const sentences: string[] = [];
  let start = 0;
  for (let i = 0; i < buf.length; i++) {
    const c = buf[i];
    if (c === "\n" || c === "." || c === "!" || c === "?") {
      const seg = buf.slice(start, i + 1).trim();
      if (c === "\n" || seg.length >= 12) {
        if (seg) sentences.push(seg);
        start = i + 1;
      }
    }
  }
  return { sentences, rest: buf.slice(start) };
}

/** Synthesize ONE sentence and queue it for in-order playback (streaming voice).
 *  Mirrors maybeSpeak's engine selection but ENQUEUES instead of replacing, and drops
 *  the audio if a stop/barge-in happened while it was still synthesizing. */
async function speakSentenceQueued(raw: string) {
  const v = useVoiceStore.getState();
  const text = speakableText(raw);
  if (!v.ttsEnabled || !text.trim()) return;
  const setSpeaking = useAgentStore.getState().setSpeaking;
  setSpeaking(true);
  const onDrain = () => setSpeaking(false);
  const epoch = currentSpeechEpoch();
  const enqueue = (b64: string, mime: string) => {
    if (currentSpeechEpoch() !== epoch) return; // stopped / barged-in mid-synth → discard
    enqueueSpeechBytes(b64, mime, onDrain);
  };
  try {
    if (v.ttsEngine === "piper") {
      return enqueue(await api.synthesize(text, v.ttsPiperVoice || "en_US-hfc_female-medium", "piper"), "audio/wav");
    }
    if (v.ttsEngine === "edge") {
      return enqueue(await api.synthesize(text, v.ttsEdgeVoice || "en-US-AriaNeural", "edge"), "audio/mpeg");
    }
    if (v.ttsEngine === "cloud") {
      return enqueue(await api.synthesize(text, v.ttsCloudVoice || "sage", "cloud", v.ttsInstructions), "audio/wav");
    }
  } catch {
    /* fall through to the OS voice */
  }
  // OS Web Speech queues utterances natively, so per-sentence speak() plays in order.
  if (currentSpeechEpoch() === epoch) {
    speak(text, { voice: v.ttsVoice || undefined, rate: v.ttsRate, onEnd: onDrain });
  }
}
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
  /** TTS is currently reading a reply aloud (so the user can press Stop to hush it). */
  speaking: boolean;
  streamStats: TokenStats | null;
  contextUsage: ContextUsage | null;
  /** Increments when conversation is auto-compacted — drives hourglass flip. */
  compactFlipCount: number;
  error: string | null;
  targets: string[];
  pendingApprovals: AgentApproval[];
  pendingQuestions: AgentQuestion[];
  pendingPlan: AgentPlan | null;
  planMode: boolean;
  hydrated: boolean;

  init: () => Promise<void>;
  setTargets: (ids: string[]) => void;
  setSpeaking: (v: boolean) => void;
  togglePlanMode: () => void;
  send: (text: string, opts?: { providerId?: string; conversation?: boolean }) => Promise<void>;
  stop: () => Promise<void>;
  newConversation: () => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  removeConversation: (id: string) => Promise<void>;
  subscribeApprovals: () => Promise<UnlistenFn>;
  resolveApproval: (id: string, approved: boolean, remember?: boolean) => Promise<void>;
  answerQuestion: (id: string, answer: string) => Promise<void>;
  resolvePlan: (id: string, approve: boolean, feedback?: string) => Promise<void>;
}

const newSessionId = () =>
  (crypto.randomUUID && crypto.randomUUID()) ||
  Math.random().toString(36).slice(2);

/** Snapshot the user's open canvas (terminals + SFTP panels) so the agent can see
 * and act on them. Terminal scrollback is fetched backend-side from session_id. */
function canvasSnapshot(): CanvasSnapshotNode[] {
  const nodes = useCanvasStore.getState().nodes;
  const sessions = useSessionStore.getState().sessions;
  return nodes.map((n) => {
    const info = sessions[n.id];
    const base = {
      node_id: n.id,
      vps_id: String(n.data.vpsId ?? ""),
      name: String(n.data.name ?? ""),
      host: String(n.data.host ?? ""),
      status: info?.status,
    };
    return n.type === "sftp"
      ? { kind: "sftp" as const, ...base, path: info?.sftpPath }
      : { kind: "terminal" as const, ...base, session_id: info?.sessionId, cwd: info?.cwd };
  });
}

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
  speaking: false,
  streamStats: null,
  contextUsage: null,
  compactFlipCount: 0,
  error: null,
  targets: [],
  pendingApprovals: [],
  pendingQuestions: [],
  pendingPlan: null,
  planMode: false,
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

  togglePlanMode: () => set((s) => ({ planMode: !s.planMode })),

  // Subscribes to all three interactive agent events (approval / question /
  // plan), each of which fires an OS notification and shows an in-chat popup.
  // Returns one combined unlisten. (Name kept for the existing App.tsx wiring.)
  subscribeApprovals: async () => {
    const unApproval = await onAgentApproval((approval) => {
      set((s) =>
        s.pendingApprovals.some((a) => a.id === approval.id)
          ? s
          : { pendingApprovals: [...s.pendingApprovals, approval] },
      );
      void notify("xConsole agent — approval needed", approval.command);
    });
    const unQuestion = await onAgentQuestion((question) => {
      set((s) =>
        s.pendingQuestions.some((q) => q.id === question.id)
          ? s
          : { pendingQuestions: [...s.pendingQuestions, question] },
      );
      const first = question.questions[0]?.question ?? "The agent has a question.";
      void notify("xConsole agent — needs your input", first);
      maybeSpeak(first);
    });
    const unPlan = await onAgentPlan((plan) => {
      set({ pendingPlan: plan });
      void notify(
        "xConsole agent — plan ready for review",
        plan.title || "Review the proposed plan.",
      );
    });
    return () => {
      unApproval();
      unQuestion();
      unPlan();
    };
  },

  resolveApproval: async (id, approved, remember) => {
    const sessionId = get().sessionId;
    set((s) => ({
      pendingApprovals: s.pendingApprovals.filter((a) => a.id !== id),
    }));
    await api.agentResolveApproval(id, approved, remember, sessionId);
  },

  answerQuestion: async (id, answer) => {
    set((s) => ({
      pendingQuestions: s.pendingQuestions.filter((q) => q.id !== id),
    }));
    await api.agentAnswerPrompt(id, answer);
  },

  resolvePlan: async (id, approve, feedback) => {
    set({ pendingPlan: null });
    const answer = approve ? "APPROVE" : `REJECT: ${feedback ?? ""}`.trim();
    await api.agentAnswerPrompt(id, answer);
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

  setSpeaking: (speaking) => set({ speaking }),

  stop: async () => {
    // Hush any spoken reply immediately…
    if (get().speaking) {
      cancelSpeech();
      set({ speaking: false });
    }
    // …and ask the running turn to stop.
    if (get().streaming) {
      await api.agentCancel(get().sessionId).catch(() => {});
    }
  },

  send: async (text, opts) => {
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
    // Session-scoped turn state. If the user switches conversations mid-stream we
    // must not read or clobber the now-visible thread, so the turn tracks its own
    // text, activity, and (post-compaction) messages locally instead of via shared
    // store state, and every shared set() is gated on still being the live session.
    let turnText = "";
    let turnActivity: AgentActivityItem[] = [];
    let turnMessages: AgentChatMessage[] = history;

    const { sessionId, targets, planMode } = get();
    const mySession = sessionId;
    const isCurrent = () => get().sessionId === mySession;

    // Streaming TTS: in hands-free voice (conversation mode) speak each sentence as
    // soon as it's generated instead of waiting for the whole reply, so the user hears
    // a response almost immediately. Gated to conversation turns (replies are markdown-
    // free there), so typed-with-TTS turns keep the safe whole-reply path below.
    const streamVoice = (opts?.conversation ?? false) && useVoiceStore.getState().ttsEnabled;
    let speechBuf = "";
    let streamingSpoke = false;
    let speechChain: Promise<void> = Promise.resolve();
    const feedSpeech = (delta: string) => {
      if (!streamVoice) return;
      speechBuf += delta;
      const ex = extractSentences(speechBuf);
      speechBuf = ex.rest;
      for (const s of ex.sentences) {
        streamingSpoke = true;
        speechChain = speechChain.then(() => speakSentenceQueued(s));
      }
    };

    const unlisten = await onAiChatOutput(mySession, (ev) => {
      if (ev.kind === "Text") {
        if (streamStartedAt === null) streamStartedAt = Date.now();
        turnText += ev.data;
        feedSpeech(ev.data);
        if (isCurrent()) {
          set({ streamingText: turnText, streamStats: liveTokenStats(turnText, streamStartedAt) });
        }
        return;
      }
      if (ev.kind === "Stats") {
        latestStats = {
          completionTokens: ev.data.completion_tokens,
          promptTokens: ev.data.prompt_tokens ?? undefined,
          tokensPerSec: ev.data.tokens_per_sec,
          source: "provider",
        };
        if (isCurrent()) set({ streamStats: latestStats });
        return;
      }
      if (ev.kind === "ContextUsage") {
        if (isCurrent()) set({ contextUsage: ev.data });
        return;
      }
      if (ev.kind === "ConversationCompacted") {
        // Preserve tool_calls / tool_call_id so the next request's history keeps
        // valid tool_use ids (dropping them 400s the Anthropic Messages API).
        turnMessages = ev.data.messages.map((m) => ({
          role: m.role as "user" | "assistant" | "system" | "tool",
          content: m.content,
          tool_calls: m.tool_calls,
          tool_call_id: m.tool_call_id,
        }));
        if (isCurrent()) {
          set((s) => ({ messages: turnMessages, compactFlipCount: s.compactFlipCount + 1 }));
        }
        return;
      }
      if (ev.kind === "Error") {
        if (isCurrent()) set({ error: ev.data });
        return;
      }
      turnActivity = applyStreamEvent(turnActivity, ev);
      if (isCurrent()) set({ activity: turnActivity });
    });

    try {
      const assistant = await api.aiChat({
        sessionId: mySession,
        messages: history,
        providerId: opts?.providerId || null,
        targets,
        planMode,
        workspaceId: useWorkspaceStore.getState().activeId,
        canvas: canvasSnapshot(),
        conversation: opts?.conversation ?? false,
      });
      const tokenStats =
        latestStats ??
        (turnText && streamStartedAt ? liveTokenStats(turnText, streamStartedAt) : undefined);
      const messages = [
        ...turnMessages,
        {
          ...assistant,
          activity: turnActivity.length > 0 ? [...turnActivity] : undefined,
          tokenStats,
        },
      ];
      if (isCurrent()) {
        set({ messages, streamingText: "", activity: [], streaming: false, streamStats: null });
        if (streamVoice && streamingSpoke) {
          // Speak whatever's left after the last sentence boundary.
          const tail = speechBuf.trim();
          if (tail) speechChain = speechChain.then(() => speakSentenceQueued(tail));
        } else {
          maybeSpeak(assistant.content);
        }
      }
      await persistConversation({ sessionId: mySession, messages, targets });
      const list = await api.listAgentConversations().catch(() => get().conversations);
      set({ conversations: list });
    } catch (e) {
      const messages = turnText
        ? [
            ...turnMessages,
            {
              role: "assistant" as const,
              content: turnText,
              activity: turnActivity.length > 0 ? [...turnActivity] : undefined,
            },
          ]
        : turnMessages;
      if (isCurrent()) {
        set({
          streaming: false,
          error: String(e),
          messages,
          streamingText: "",
          activity: [],
          streamStats: null,
        });
      }
      if (messages.length > 0) {
        await persistConversation({ sessionId: mySession, messages, targets }).catch(() => {});
      }
    } finally {
      unlisten();
    }
  },
}));
