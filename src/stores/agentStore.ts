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
  type AgentPlanMeta,
  type AgentQuestion,
  type CanvasSnapshotNode,
  type ChatMessage,
} from "../lib/tauri";
import { appendImageMarkers } from "../lib/vision";
import { notify } from "../lib/notify";
import { exportConversationMarkdown as renderConversationMarkdown } from "../lib/agentExport";
import { classifyChat } from "../lib/consent";
import { useWorkspaceStore } from "./workspaceStore";
import { useCanvasStore } from "./canvasStore";
import { useSessionStore } from "./sessionStore";
import { useVoiceStore } from "./voiceStore";
import { useSettingsStore } from "./settingsStore";
import { contextUsageFromMessages, type PrefixTelemetry, type TurnTelemetry } from "../lib/streamStats";
import {
  appendTextDelta,
  applyActivityEvent,
  flattenActivity,
  textFromSegments,
  type TurnSegment,
} from "./turnSegments";
import {
  enqueueMessage,
  removeQueuedMessage,
  takeNextQueued,
  updateQueuedMessage,
  type QueuedMessage,
} from "./messageQueue";

export type { QueuedMessage } from "./messageQueue";

export type { TurnSegment } from "./turnSegments";
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
  estimateTokens,
  liveGenerationStats,
  liveTokenStats,
  sessionCostFromMessages,
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
export type { TokenStats, ContextUsage, TurnTelemetry } from "../lib/streamStats";

/** Format execution duration cleanly e.g. "Worked for 42s", "Worked for 2m 15s", "Worked for 1h 12m 4s". */
export function formatWorkingDuration(ms: number, prefix: string = "Worked for"): string {
  const totalSeconds = Math.max(1, Math.round(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  let timeStr = "";
  if (hours > 0) {
    if (minutes > 0) {
      timeStr = `${hours}h ${minutes}m ${seconds}s`;
    } else {
      timeStr = `${hours}h ${seconds}s`;
    }
  } else if (minutes > 0) {
    timeStr = `${minutes}m ${seconds}s`;
  } else {
    timeStr = `${seconds}s`;
  }

  return prefix ? `${prefix} ${timeStr}` : timeStr;
}

export interface AgentChatMessage extends ChatMessage {
  activity?: AgentActivityItem[];
  /** Chronological text / tool bursts for this turn. Prefer this when rendering. */
  segments?: TurnSegment[];
  tokenStats?: TokenStats;
  /** Milliseconds the turn took from start to finish. */
  durationMs?: number;
  /** Formatted duration string e.g. "Worked for 42s" / "Worked for 2m 15s" */
  durationFormatted?: string;
  isCompaction?: boolean;
  compactionTokensBefore?: number;
  compactionTokensAfter?: number;
  compactionPrunedTools?: number;
  compactionSummary?: string;
}

import { resolveEffectiveMode, type AgentRuntimeMode } from "../lib/agentMode";
export type { AgentRuntimeMode };

interface AgentState {
  sessionId: string;
  messages: AgentChatMessage[];
  conversations: AgentConversationMeta[];
  streamingText: string;
  activity: AgentActivityItem[];
  /** Live turn timeline (text then tools then more text). */
  streamingSegments: TurnSegment[];
  streaming: boolean;
  /** Epoch timestamp (Date.now()) when the active turn started. */
  turnStartTime: number | null;
  /** TTS is currently reading a reply aloud (so the user can press Stop to hush it). */
  speaking: boolean;
  streamStats: TokenStats | null;
  turnTelemetry: TurnTelemetry | null;
  prefixTelemetry: PrefixTelemetry | null;
  contextUsage: ContextUsage | null;
  /** Increments when conversation is auto-compacted — drives hourglass flip. */
  compactFlipCount: number;
  error: string | null;
  targets: string[];
  pendingApprovals: AgentApproval[];
  pendingQuestions: AgentQuestion[];
  pendingPlan: AgentPlan | null;
  /** Text of previous plan before revision, used for git-diff view in plan modal. */
  previousPlanText: string | null;
  /** Editable copy of the pending plan shown in the modal. */
  planDraft: string;
  /** Persisted plan history (presented/applied/archived/cancelled). */
  planHistory: AgentPlanMeta[];
  planHistoryOpen: boolean;
  /** True ONLY when the user sent feedback asking the agent to revise the plan. */
  planRevising: boolean;
  planMode: boolean;
  /** DeepSeek Harness-inspired runtime mode: standard, code, plan, or minimal. */
  agentMode: AgentRuntimeMode;
  hydrated: boolean;
  /** Running estimated cost (USD) of the current conversation. */
  conversationCostUsd: number;
  /** Follow-ups typed while a turn is running. Editable until they send. */
  queued: QueuedMessage[];
  /** After Stop, do not auto-send the queue until the user sends again. */
  holdQueue: boolean;
  /** Intake `/goal` session the chat tools should bind to. */
  activeIntakeGoalId: string | null;

  init: () => Promise<void>;
  setTargets: (ids: string[]) => void;
  setSpeaking: (v: boolean) => void;
  togglePlanMode: () => void;
  setAgentMode: (mode: AgentRuntimeMode) => void;
  setActiveIntakeGoal: (id: string | null) => void;
  send: (
    text: string,
    opts?: {
      providerId?: string;
      conversation?: boolean;
      images?: import("../lib/tauri").ChatImage[];
      goalId?: string;
    },
  ) => Promise<void>;
  /** Queue a follow-up if a turn is running; otherwise send now. */
  enqueueOrSend: (text: string, images?: import("../lib/tauri").ChatImage[]) => void;
  /**
   * If a plan / question / command-approval is waiting, resolve it from
   * this chat line and return true. Used by send + enqueueOrSend.
   */
  tryRouteChatToPending: (text: string) => boolean;
  enqueue: (text: string, images?: import("../lib/tauri").ChatImage[]) => void;
  updateQueued: (id: string, text: string) => void;
  removeQueued: (id: string) => void;
  /** Re-send the last user message (after an error or aborted turn). */
  retryLast: () => Promise<void>;
  clearError: () => void;
  stop: () => Promise<void>;
  newConversation: () => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  removeConversation: (id: string) => Promise<void>;
  renameConversation: (id: string, title: string) => Promise<void>;
  /** Export the current conversation as Markdown (clipboard + optional download). */
  exportConversationMarkdown: () => string;
  subscribeApprovals: () => Promise<UnlistenFn>;
  resolveApproval: (id: string, approved: boolean, remember?: boolean) => Promise<void>;
  answerQuestion: (id: string, answer: string) => Promise<void>;
  resolvePlan: (id: string, approve: boolean, feedback?: string) => Promise<void>;
  /** Modal actions: keep the plan open while the agent revises it. */
  setPlanDraft: (draft: string) => void;
  applyPlan: (id: string) => Promise<void>;
  archivePlanAction: (id: string) => Promise<void>;
  cancelPlanAction: (id: string) => Promise<void>;
  /** Send revision feedback to the agent (plan modal chat). */
  revisePlan: (id: string, feedback: string) => Promise<void>;
  loadPlanHistory: () => Promise<void>;
  setPlanHistoryOpen: (open: boolean) => void;
  closePlanModal: () => Promise<void>;
  stopPlanRevision: () => Promise<void>;
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
    if (n.type === "sftp") {
      return {
        kind: "sftp" as const,
        ...base,
        path: info?.sftpPath,
        git_branch: info?.gitBranch ?? null,
        git_dirty: info?.gitDirty ?? null,
      };
    }
    // Don't describe a database browser as a terminal — the agent would offer to run
    // shell commands in it. The Rust renderer ignores kinds it doesn't know.
    if (n.type === "db") {
      return { kind: "db" as const, ...base };
    }
    return {
      kind: "terminal" as const,
      ...base,
      session_id: info?.sessionId,
      cwd: info?.cwd,
      git_branch: info?.gitBranch ?? null,
      git_dirty: info?.gitDirty ?? null,
    };
  });
}

function persistableMessages(messages: AgentChatMessage[]): AgentChatMessage[] {
  let lastUser = -1;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === "user") {
      lastUser = i;
      break;
    }
  }
  return messages.map((m, i) => {
    if (i === lastUser || !m.images?.length) return m;
    const { images: _drop, ...rest } = m;
    return rest;
  });
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
    messagesJson: JSON.stringify(persistableMessages(state.messages)),
  });
}

export const useAgentStore = create<AgentState>((set, get) => ({
  sessionId: newSessionId(),
  messages: [],
  conversations: [],
  streamingText: "",
  activity: [],
  streamingSegments: [],
  streaming: false,
  turnStartTime: null,
  speaking: false,
  streamStats: null,
  turnTelemetry: null,
  prefixTelemetry: null,
  contextUsage: null,
  compactFlipCount: 0,
  error: null,
  queued: [],
  holdQueue: false,
  activeIntakeGoalId: null,
  targets: (() => {
    try {
      const raw = localStorage.getItem("xconsole-agent-targets");
      if (!raw) return [] as string[];
      const parsed = JSON.parse(raw) as unknown;
      return Array.isArray(parsed) ? (parsed as string[]) : [];
    } catch {
      return [] as string[];
    }
  })(),
  pendingApprovals: [],
  pendingQuestions: [],
  pendingPlan: null,
  previousPlanText: null,
  planDraft: "",
  planHistory: [],
  planHistoryOpen: false,
  planRevising: false,
  planMode:
    typeof localStorage !== "undefined" &&
    localStorage.getItem("xconsole-agent-plan-mode") === "1",
  agentMode:
    (typeof localStorage !== "undefined" &&
      (localStorage.getItem("xconsole-agent-mode") as AgentRuntimeMode)) ||
    "auto",
  hydrated: false,
  conversationCostUsd: 0,

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

  setTargets: (ids) => {
    try {
      localStorage.setItem("xconsole-agent-targets", JSON.stringify(ids));
    } catch {
      /* ignore */
    }
    set({ targets: ids });
  },

  togglePlanMode: () =>
    set((s) => {
      const nextPlan = !s.planMode;
      const agentMode: AgentRuntimeMode = nextPlan ? "plan" : "auto";
      try {
        localStorage.setItem("xconsole-agent-plan-mode", nextPlan ? "1" : "0");
        localStorage.setItem("xconsole-agent-mode", agentMode);
      } catch {
        /* ignore */
      }
      return { planMode: nextPlan, agentMode };
    }),

  setAgentMode: (mode) => {
    try {
      localStorage.setItem("xconsole-agent-mode", mode);
      localStorage.setItem("xconsole-agent-plan-mode", mode === "plan" ? "1" : "0");
    } catch {
      /* ignore */
    }
    set({ agentMode: mode, planMode: mode === "plan" });
  },

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
      const current = get().pendingPlan?.plan;
      set((s) => ({
        pendingPlan: plan,
        planDraft: plan.plan,
        previousPlanText: current && current !== plan.plan ? current : s.previousPlanText,
        planRevising: false,
      }));
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
    const denied = get().pendingApprovals.find((a) => a.id === id);
    set((s) => ({
      pendingApprovals: s.pendingApprovals.filter((a) => a.id !== id),
    }));
    await api.agentResolveApproval(id, approved, remember, sessionId);
    // Taste learning: user denied a command → remember not to auto-run that class of action.
    if (!approved && denied?.command) {
      try {
        const docs = await api.getAgentDocs();
        const cmd = denied.command.trim().slice(0, 120);
        const bullet = `- [taste] Do not run without approval: ${cmd}`;
        if (!docs.taste.includes(cmd)) {
          const next = docs.taste.trim()
            ? `${docs.taste.trim()}\n${bullet}\n`
            : `${bullet}\n`;
          await api.saveTasteDoc(next);
        }
      } catch {
        /* non-fatal */
      }
    }
  },

  answerQuestion: async (id, answer) => {
    set((s) => ({
      pendingQuestions: s.pendingQuestions.filter((q) => q.id !== id),
    }));
    await api.agentAnswerPrompt(id, answer);
  },

  resolvePlan: async (id, approve, feedback) => {
    set({ pendingPlan: null, previousPlanText: null, planDraft: "", planRevising: false });
    const answer = approve ? "APPROVE" : `REJECT: ${feedback ?? ""}`.trim();
    await api.agentAnswerPrompt(id, answer);
    // Taste learning: plan rejection feedback becomes a lasting preference.
    if (!approve && feedback?.trim()) {
      try {
        const docs = await api.getAgentDocs();
        const bullet = `- [taste] Plan feedback: ${feedback.trim()}`;
        if (!docs.taste.includes(feedback.trim())) {
          const next = docs.taste.trim()
            ? `${docs.taste.trim()}\n${bullet}\n`
            : `${bullet}\n`;
          await api.saveTasteDoc(next);
        }
      } catch {
        /* non-fatal */
      }
    }
  },

  setPlanDraft: (draft) => set({ planDraft: draft }),

  applyPlan: async (id) => {
    const draft = get().planDraft || get().pendingPlan?.plan;
    set({ pendingPlan: null, previousPlanText: null, planDraft: "", planRevising: false });
    try {
      await api.agentAnswerPrompt(id, "APPROVE");
    } catch {
      // If the backend prompt was cancelled/stopped or timed out, trigger execution as a new turn!
      if (draft?.trim()) {
        void get().send(`Proceed and execute this approved plan:\n\n${draft}`);
      }
    }
    void get().loadPlanHistory();
  },

  archivePlanAction: async (id) => {
    try {
      await api.archivePlan(id);
    } catch (e) {
      throw e;
    }
    set({ pendingPlan: null, previousPlanText: null, planDraft: "", planRevising: false });
    void get().loadPlanHistory();
  },

  cancelPlanAction: async (id) => {
    try {
      await api.cancelPlan(id);
    } catch (e) {
      throw e;
    }
    set({ pendingPlan: null, previousPlanText: null, planDraft: "", planRevising: false });
    void get().loadPlanHistory();
  },

  revisePlan: async (id, feedback) => {
    set({ planRevising: true });
    try {
      // Keep the modal open; the agent revises and re-presents (new ai://plan).
      await api.agentAnswerPrompt(id, `REJECT: ${feedback}`.trim());
    } catch {
      // If the prompt waiter is gone (e.g. user stopped agent), send feedback as a new turn to trigger revision.
      void get().send(`Please revise the plan based on this feedback:\n\n${feedback}`);
    }
  },

  loadPlanHistory: async () => {
    try {
      const sid = get().sessionId;
      const wid = useWorkspaceStore.getState().activeId;
      const history = await api.listPlans(sid ?? null, wid ?? null);
      set({ planHistory: history });
    } catch {
      /* non-fatal */
    }
  },

  setPlanHistoryOpen: (open) => set({ planHistoryOpen: open }),

  closePlanModal: async () => {
    const pending = get().pendingPlan;
    set({ pendingPlan: null, previousPlanText: null, planDraft: "", planHistoryOpen: false, planRevising: false });
    // Closing without a decision = cancel: unblock the waiting present_plan
    // tool and record the plan as cancelled in history.
    if (pending) {
      await api.cancelPlan(pending.id).catch(() => {});
      void get().loadPlanHistory();
    }
  },

  stopPlanRevision: async () => {
    if (get().speaking) {
      cancelSpeech();
      set({ speaking: false });
    }
    if (get().streaming) {
      void api.agentCancel(get().sessionId).catch(() => {});
    }
    set({
      streaming: false,
      planRevising: false,
      holdQueue: true,
    });
  },

  newConversation: async () => {
    const id = newSessionId();
    set({
      sessionId: id,
      messages: [],
      streamingText: "",
      activity: [],
      streamingSegments: [],
      streaming: false,
      streamStats: null,
      turnTelemetry: null,
      prefixTelemetry: null,
      contextUsage: null,
      compactFlipCount: 0,
      error: null,
      conversationCostUsd: 0,
      queued: [],
      holdQueue: false,
      activeIntakeGoalId: null,
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
    const settings = useSettingsStore.getState().settings;
    const providers = useSettingsStore.getState().providers;
    const activeProvider = providers.find((p) => p.id === settings["agent.active_provider"]);
    const activeModel = settings["agent.active_model"] || activeProvider?.model;
    const initialContextUsage = contextUsageFromMessages(
      messages,
      activeProvider?.kind ?? undefined,
      activeModel ?? undefined,
    );

    set({
      sessionId: id,
      messages,
      targets,
      streamingText: "",
      activity: [],
      streamingSegments: [],
      streaming: false,
      streamStats: [...messages].reverse().find((m) => m.tokenStats)?.tokenStats ?? null,
      turnTelemetry: null,
      prefixTelemetry: null,
      contextUsage: initialContextUsage,
      compactFlipCount: 0,
      error: null,
      conversationCostUsd: sessionCostFromMessages(messages),
      queued: [],
      holdQueue: false,
      activeIntakeGoalId: null,
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

  renameConversation: async (id, title) => {
    const t = title.trim();
    if (!t) return;
    const conv = await api.getAgentConversation(id);
    if (!conv) return;
    let targets: string[] = [];
    try {
      targets = conv.targets_json
        ? (JSON.parse(conv.targets_json) as string[])
        : [];
    } catch {
      targets = get().targets;
    }
    await api.saveAgentConversation({
      id,
      title: t,
      targets,
      messagesJson: conv.messages_json,
    });
    const list = await api.listAgentConversations().catch(() => get().conversations);
    set({ conversations: list });
  },

  exportConversationMarkdown: () => {
    const { messages, conversations, sessionId } = get();
    const meta = conversations.find((c) => c.id === sessionId);
    return renderConversationMarkdown({ title: meta?.title, messages });
  },

  setSpeaking: (speaking) => set({ speaking }),

  setActiveIntakeGoal: (id) => set({ activeIntakeGoalId: id }),

  tryRouteChatToPending: (text) => {
    const trimmed = text.trim();
    if (!trimmed) return false;
    const s = get();
    const intent = classifyChat(trimmed);

    if (s.pendingPlan) {
      const id = s.pendingPlan.id;
      const fallbackSend = () => {
        // Waiter already gone (timeout / superseded). Drop the stale modal
        // and start a normal turn so the chat line is not swallowed.
        set({ pendingPlan: null, planDraft: "" });
        void get().send(trimmed);
      };
      if (intent.kind === "approve" || intent.kind === "continue") {
        void get().applyPlan(id).catch(fallbackSend);
        return true;
      }
      if (intent.kind === "reject") {
        void get()
          .revisePlan(id, intent.feedback || trimmed)
          .catch(fallbackSend);
        return true;
      }
      if (intent.kind === "cancel") {
        void get().cancelPlanAction(id).catch(fallbackSend);
        return true;
      }
      // Free-form notes while the modal is open are revision feedback,
      // not a second agent turn that would sit behind streaming.
      void get().revisePlan(id, trimmed).catch(fallbackSend);
      return true;
    }

    if (s.pendingQuestions.length > 0) {
      const q = s.pendingQuestions[0];
      void get().answerQuestion(q.id, trimmed);
      return true;
    }

    if (s.pendingApprovals.length > 0) {
      if (intent.kind === "approve" || intent.kind === "continue") {
        void get().resolveApproval(s.pendingApprovals[0].id, true);
        return true;
      }
      if (intent.kind === "reject" || intent.kind === "cancel") {
        void get().resolveApproval(s.pendingApprovals[0].id, false);
        return true;
      }
    }
    return false;
  },

  enqueue: (text, images) => {
    set((s) => ({ queued: enqueueMessage(s.queued, text, images) }));
  },

  updateQueued: (id, text) => {
    set((s) => ({ queued: updateQueuedMessage(s.queued, id, text) }));
  },

  removeQueued: (id) => {
    set((s) => ({ queued: removeQueuedMessage(s.queued, id) }));
  },

  enqueueOrSend: (text, images) => {
    const trimmed = text.trim();
    if (!trimmed && !images?.length) return;
    // Chat typed while a plan / question / command-approval is waiting
    // must resolve that waiter. Queueing it as a follow-up is how
    // "ok the plan looks good" used to vanish into a new blocked turn.
    if (get().tryRouteChatToPending(trimmed)) return;
    if (get().streaming) {
      get().enqueue(trimmed, images);
      return;
    }
    void get().send(trimmed, { images });
  },

  stop: async () => {
    // Hush any spoken reply immediately…
    if (get().speaking) {
      cancelSpeech();
      set({ speaking: false });
    }
    // …and ask the running turn to stop (backend also interrupts any
    // interactive plan/question wait for this session).
    if (get().streaming) {
      void api.agentCancel(get().sessionId).catch(() => {});
    }
    // Clear any pending interactive state so the modal/cards don't linger.
    // Immediately clear streaming on frontend so the Stop button instantly responds.
    set({
      streaming: false,
      pendingPlan: null,
      pendingQuestions: [],
      pendingApprovals: [],
      planDraft: "",
      holdQueue: true,
    });
  },

  send: async (text, opts) => {
    const trimmed = text.trim();
    const images = opts?.images?.length ? opts.images : undefined;
    if (!trimmed && !images) return;
    // Same routing as enqueueOrSend — voice / retry / queue-drain call send
    // directly. If a waiter is up, resolve it instead of dropping or
    // starting a second turn.
    if (trimmed && get().tryRouteChatToPending(trimmed)) return;
    if (get().streaming) return;

    const userMsg: AgentChatMessage = {
      role: "user",
      content: appendImageMarkers(trimmed, images?.length ?? 0),
      images,
    };
    const turnStartTime = Date.now();
    const history = [...get().messages, userMsg];
    set({
      messages: history,
      streaming: true,
      turnStartTime,
      holdQueue: false,
      streamingText: "",
      activity: [],
      streamingSegments: [],
      streamStats: null,
      turnTelemetry: null,
      error: null,
    });

    let streamStartedAt: number | null = null;
    let lastStatsAt = 0;
    let tokensBeforeBurst = 0;
    let latestStats: TokenStats | null = null;
    // Session-scoped turn state. If the user switches conversations mid-stream we
    // must not read or clobber the now-visible thread, so the turn tracks its own
    // text, activity, and (post-compaction) messages locally instead of via shared
    // store state, and every shared set() is gated on still being the live session.
    let turnText = "";
    let turnActivity: AgentActivityItem[] = [];
    let turnSegments: TurnSegment[] = [];

    const { sessionId, targets, planMode, agentMode } = get();
    const effectiveMode = resolveEffectiveMode(agentMode, trimmed);
    const effectivePlanMode = planMode || effectiveMode === "plan";
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

    let compactionMarker: AgentChatMessage | null = null;

    // Stream deltas arrive one per token. Writing each straight to the store made
    // every subscriber re-render per token — and the console renders `streamingText`
    // through react-markdown + highlight.js, so a long reply re-parsed the whole
    // growing document hundreds of times and visibly crawled.
    //
    // Deltas are already accumulated in the local `turnText` / `turnSegments` above,
    // so the store only needs the latest value, not every intermediate one. Coalesce
    // into at most one write per animation frame: nothing the user can perceive is
    // lost (the display cannot show more than one frame anyway) and the render cost
    // stops scaling with token count.
    let flushHandle: number | null = null;
    let pendingText = false;
    let pendingActivity = false;
    let pendingStats: TokenStats | null = null;

    const flushNow = () => {
      flushHandle = null;
      if (!pendingText && !pendingActivity && !pendingStats) return;
      const update: Partial<AgentState> = {};
      if (pendingText) update.streamingText = turnText;
      if (pendingText || pendingActivity) update.streamingSegments = turnSegments;
      if (pendingActivity) update.activity = turnActivity;
      if (pendingStats) update.streamStats = pendingStats;
      pendingText = false;
      pendingActivity = false;
      pendingStats = null;
      if (isCurrent()) set(update);
    };

    const scheduleFlush = () => {
      if (flushHandle !== null) return;
      flushHandle = requestAnimationFrame(flushNow);
    };

    // Drop anything still queued. The turn is about to write its final state, and a
    // frame landing after that would restore stale streaming text over the finished
    // message. Idempotent, so calling it more than once is fine.
    const cancelFlush = () => {
      if (flushHandle !== null) {
        cancelAnimationFrame(flushHandle);
        flushHandle = null;
      }
      pendingText = false;
      pendingActivity = false;
      pendingStats = null;
    };

    const unlisten = await onAiChatOutput(mySession, (ev) => {
      if (ev.kind === "Text") {
        if (streamStartedAt === null) {
          streamStartedAt = Date.now();
          tokensBeforeBurst = estimateTokens(turnText);
        }
        turnText += ev.data;
        turnSegments = appendTextDelta(turnSegments, ev.data);
        feedSpeech(ev.data);
        if (isCurrent()) {
          const now = Date.now();
          const shouldUpdateStats = !lastStatsAt || now - lastStatsAt >= 200;
          if (shouldUpdateStats) {
            lastStatsAt = now;
            const live = liveGenerationStats(turnText, streamStartedAt, tokensBeforeBurst);
            const curStats = get().streamStats;
            pendingStats =
              curStats?.source === "provider"
                ? {
                    ...curStats,
                    completionTokens: live.completionTokens,
                    tokensPerSec: live.tokensPerSec,
                  }
                : live;
          }
          pendingText = true;
          scheduleFlush();
        }
        return;
      }
      // Next text burst (after tools) gets a fresh generation clock so tok/s
      // does not include SSH/tool wait time.
      if (ev.kind === "ToolCall" || ev.kind === "Activity" || ev.kind === "Status") {
        streamStartedAt = null;
      }
      if (ev.kind === "Stats") {
        latestStats = {
          completionTokens: ev.data.completion_tokens,
          promptTokens: ev.data.prompt_tokens ?? undefined,
          cachedTokens: ev.data.cached_tokens ?? undefined,
          cacheCreationTokens: ev.data.cache_creation_tokens ?? undefined,
          tokensPerSec: ev.data.tokens_per_sec,
          source: "provider",
        };
        if (isCurrent()) set({ streamStats: latestStats });
        return;
      }
      if (ev.kind === "Cost") {
        // Provider-estimated cost lands on the latest stats so the footer can show
        // $ + cache economics. Sums into the conversation running total.
        const costUsd = ev.data.usd ?? 0;
        if (latestStats) latestStats = { ...latestStats, costUsd };
        if (isCurrent()) {
          set((s) => ({
            streamStats: s.streamStats ? { ...s.streamStats, costUsd } : s.streamStats,
            conversationCostUsd: (s.conversationCostUsd ?? 0) + costUsd,
          }));
        }
        return;
      }
      if (ev.kind === "TurnTelemetry") {
        const turnTelemetry: TurnTelemetry = {
          toolCalls: ev.data.tool_calls,
          toolCacheLookups: ev.data.tool_cache_lookups,
          toolCacheHits: ev.data.tool_cache_hits,
          toolCacheMisses: ev.data.tool_cache_misses,
          toolCacheWrites: ev.data.tool_cache_writes,
          toolCacheHitRate: ev.data.tool_cache_hit_rate,
        };
        if (isCurrent()) set({ turnTelemetry });
        return;
      }
      if (ev.kind === "PrefixTelemetry") {
        const prefixTelemetry: PrefixTelemetry = {
          requestIndex: ev.data.request_index,
          systemHash: ev.data.system_hash,
          schemaHash: ev.data.schema_hash,
          messagePrefixHash: ev.data.message_prefix_hash,
          systemBytes: ev.data.system_bytes,
          schemaBytes: ev.data.schema_bytes,
          messageBytes: ev.data.message_bytes,
          classification: ev.data.classification,
          source: ev.data.source,
        };
        if (isCurrent()) set({ prefixTelemetry });
        return;
      }
      if (ev.kind === "ContextUsage") {
        if (isCurrent()) set({ contextUsage: ev.data });
        return;
      }
      if (ev.kind === "ConversationCompacted") {
        compactionMarker = {
          role: "system",
          content: "Context compacted",
          isCompaction: true,
          compactionTokensBefore: (ev.data as any)?.tokens_before,
          compactionTokensAfter: (ev.data as any)?.tokens_after,
          compactionPrunedTools: (ev.data as any)?.pruned_tools,
        };
        if (isCurrent()) {
          set((s) => ({
            compactFlipCount: s.compactFlipCount + 1,
          }));
        }
        return;
      }
      if (ev.kind === "Error") {
        if (isCurrent()) set({ error: ev.data });
        return;
      }
      turnSegments = applyActivityEvent(turnSegments, ev);
      turnActivity = flattenActivity(turnSegments);
      if (isCurrent()) {
        pendingActivity = true;
        scheduleFlush();
      }
    });

    try {
      const assistant = await api.aiChat({
        sessionId: mySession,
        messages: history,
        providerId: opts?.providerId || null,
        targets,
        planMode: effectivePlanMode,
        workspaceId: useWorkspaceStore.getState().activeId,
        canvas: canvasSnapshot(),
        conversation: opts?.conversation ?? false,
        goalId: opts?.goalId ?? get().activeIntakeGoalId,
      });
      cancelFlush();
      const tokenStats =
        latestStats ??
        (turnText && streamStartedAt ? liveTokenStats(turnText, streamStartedAt) : undefined);
      const turnDurationMs = turnStartTime ? Date.now() - turnStartTime : undefined;
      const turnDurationFormatted = turnDurationMs ? formatWorkingDuration(turnDurationMs) : undefined;
      const messages: AgentChatMessage[] = [
        ...history,
        ...(compactionMarker ? [compactionMarker] : []),
        {
          ...assistant,
          content: assistant.content || textFromSegments(turnSegments),
          activity: turnActivity.length > 0 ? [...turnActivity] : undefined,
          segments: turnSegments.length > 0 ? [...turnSegments] : undefined,
          tokenStats,
          durationMs: turnDurationMs,
          durationFormatted: turnDurationFormatted,
        },
      ];
      if (isCurrent()) {
        set({
          messages,
          streamingText: "",
          activity: [],
          streamingSegments: [],
          streaming: false,
          turnStartTime: null,
          // Keep the last reply's usage so cache rates do not vanish after the turn.
          streamStats: tokenStats ?? null,
        });
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
      if (isCurrent() && !get().holdQueue && !get().streaming) {
        const { next, rest } = takeNextQueued(get().queued);
        if (next) {
          set({ queued: rest });
          void get().send(next.text, { images: next.images });
        }
      }
    } catch (e) {
      cancelFlush();
      const hasAssistantContent =
        Boolean(turnText.trim()) ||
        turnActivity.length > 0 ||
        turnSegments.length > 0;
      const assistantMsg: AgentChatMessage | null = hasAssistantContent
        ? {
            role: "assistant" as const,
            content: turnText,
            activity: turnActivity.length > 0 ? [...turnActivity] : undefined,
            segments: turnSegments.length > 0 ? [...turnSegments] : undefined,
            tokenStats: latestStats ?? undefined,
          }
        : null;
      const messages: AgentChatMessage[] = [
        ...history,
        ...(compactionMarker ? [compactionMarker] : []),
        ...(assistantMsg ? [assistantMsg] : []),
      ];
      if (isCurrent()) {
        set({
          streaming: false,
          error: String(e),
          messages,
          streamingText: "",
          activity: [],
          streamingSegments: [],
          streamStats: latestStats,
        });
      }
      if (messages.length > 0) {
        await persistConversation({ sessionId: mySession, messages, targets }).catch(() => {});
      }
    } finally {
      cancelFlush();
      unlisten();
    }
  },

  clearError: () => set({ error: null }),

  retryLast: async () => {
    if (get().streaming) return;
    const msgs = get().messages;
    if (msgs.length === 0) return;

    const lastMsg = msgs[msgs.length - 1];
    const hasAssistantWork =
      lastMsg.role === "assistant" &&
      (Boolean(lastMsg.content?.trim()) ||
        Boolean(lastMsg.activity && lastMsg.activity.length > 0) ||
        Boolean(lastMsg.segments && lastMsg.segments.length > 0));

    if (hasAssistantWork) {
      // The assistant already did research, generated a plan, or executed partial steps.
      // Do NOT wipe the conversation! Send a continuation prompt so the agent continues
      // using the established context, research, and plan.
      const err = get().error;
      set({ error: null });
      const prompt = err
        ? `The previous turn encountered an error: ${err}. Please continue from where you left off, using the context, research, and plan already established. Do not repeat already completed steps.`
        : `Please continue from where you left off. Do not repeat already completed steps.`;
      await get().send(prompt);
      return;
    }

    // If no assistant work was produced (e.g. failed immediately before any tools or output):
    let lastUserIdx = -1;
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].role === "user") {
        lastUserIdx = i;
        break;
      }
    }
    if (lastUserIdx < 0) return;
    const text = msgs[lastUserIdx].content;
    const images = msgs[lastUserIdx].images;
    // Drop the failed user turn and any trailing empty assistant so send() re-appends cleanly.
    set({ messages: msgs.slice(0, lastUserIdx), error: null });
    await get().send(text, { images });
  },
}));
