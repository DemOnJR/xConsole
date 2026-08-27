import { type AgentApproval, type AgentConversationMeta, type AgentPlan, type AgentPlanMeta, type AgentQuestion, type ChatMessage } from "../lib/tauri";
import { type PrefixTelemetry, type TurnTelemetry } from "../lib/streamStats";
import { type TurnSegment } from "./turnSegments";
import { type QueuedMessage } from "./messageQueue";
export type { QueuedMessage } from "./messageQueue";
export type { TurnSegment } from "./turnSegments";
import { type ContextUsage, type TokenStats } from "../lib/streamStats";
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
    hunks?: {
        kind: string;
        text: string;
    }[];
}
/** Token throughput for a completed or streaming assistant message. */
export type { TokenStats, ContextUsage, TurnTelemetry } from "../lib/streamStats";
/** Format execution duration cleanly e.g. "Worked for 42s", "Worked for 2m 15s", "Worked for 1h 12m 4s". */
export declare function formatWorkingDuration(ms: number, prefix?: string): string;
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
import { type AgentRuntimeMode } from "../lib/agentMode";
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
    send: (text: string, opts?: {
        providerId?: string;
        conversation?: boolean;
        images?: import("../lib/tauri").ChatImage[];
        goalId?: string;
    }) => Promise<void>;
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
export declare const useAgentStore: import("zustand").UseBoundStore<import("zustand").StoreApi<AgentState>>;
//# sourceMappingURL=agentStore.d.ts.map