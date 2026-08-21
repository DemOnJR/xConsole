import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { NodeResizer, useStore, type NodeProps } from "@xyflow/react";

import { useAgentStore } from "../../stores/agentStore";

import { useInputHistory } from "../../hooks/useInputHistory";

import { useVoiceStore } from "../../stores/voiceStore";

import {
  startConversation,
  cancelSpeech,
  isSpeaking,
  type Conversation,
} from "../../lib/voice";

import { api, onGoalEvent, type ChatImage, type GoalSession, type GoalSpec } from "../../lib/tauri";
import { parseGoalSpec } from "../../lib/goalParse";
import {
  displayTurnStats,
  formatSessionCache,
  sessionCacheFromMessages,
} from "../../lib/streamStats";
import { clipboardImagePng } from "../../lib/terminalClipboard";
import { onOsFilesDropped } from "../../hooks/useOsFileDrop";
import {
  bytesToChatImage,
  clipboardLooksLikeImage,
  defaultVisionModel,
  fileBaseName,
  fileToChatImage,
  imagesFromClipboardEvent,
  isGeminiProvider,
  isImagePath,
  parseVisionMode,
  previewSrc,
  visionLabel,
} from "../../lib/vision";

import { useUiStore } from "../../stores/uiStore";
import { useMaskHost } from "../../lib/privacy";

import { useVpsStore } from "../../stores/vpsStore";
import { useCanvasStore, NODE_W, NODE_H, type AgentNode as AgentNodeType } from "../../stores/canvasStore";

import { useSettingsStore } from "../../stores/settingsStore";
import { dialog } from "../../stores/dialogStore";

import { AgentConsole } from "./AgentConsole";
import { GoalLockCard } from "./GoalLockCard";
import { CLIPicker, type CLIPickerOption } from "./CLIPicker";
import {
  filterSlashCommands,
  isSlashInput,
  parseExactSlashCommand,
  KEYBINDS,
  SLASH_COMMANDS,
  type SlashCommandDef,
} from "./agentCommands";
import { notify } from "../../lib/notify";
import { catalogForProvider } from "../../lib/providerCatalog";
import { ImageIcon } from "../icons";
import { createChatSnippet, shouldCreateSnippet, type ChatSnippet } from "../../lib/snippetDetect";
import { SnippetPreviewModal } from "./SnippetPreviewModal";
import { TrajectoryModal } from "./TrajectoryModal";
import { InputBar, type ReasoningLevel } from "./InputBar";
import { QueuedMessages } from "./QueuedMessages";
import { useGitBranch } from "../../hooks/useGitBranch";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { useGoalStore } from "../../stores/goalStore";
import { effectiveMode, shouldAutoRun } from "../../lib/safety";

import type { AgentApproval, AgentQuestion } from "../../lib/tauri";

function formatSessionDate(s?: string | null): string {
  if (!s) return "";
  const iso = /^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}/.test(s) ? `${s.replace(" ", "T")}Z` : s;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return s;
  const dateStr = d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const timeStr = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false });
  return `${dateStr} · ${timeStr}`;
}

/** Maximize the agent node to the whole canvas pane, or restore its default size.
 *  Shared with the NavRail double-click, which has no React access to the node. */
export function toggleAgentFillPane(id: string) {
  const canvas = useCanvasStore.getState();
  const node = canvas.nodes.find((n) => n.id === id);
  if (!node) return;
  const pane = canvas.paneSize;
  const w = Number(node.width) || NODE_W;
  const h = Number(node.height) || NODE_H;
  const fillsPane =
    pane && w >= pane.width - 4 && h >= pane.height - 4 && node.position.x <= 4 && node.position.y <= 4;
  if (fillsPane) {
    useCanvasStore.setState((s) => ({
      nodes: s.nodes.map((n) =>
        n.id === id
          ? { ...n, position: { x: 80, y: 80 }, width: NODE_W, height: NODE_H }
          : n,
      ),
    }));
    if (canvas.layoutMode === "tile") useCanvasStore.getState().arrangeTiles();
    return;
  }
  if (canvas.layoutMode === "tile") {
    useCanvasStore.getState().toggleTileFullWidth(id);
    return;
  }
  if (pane) {
    useCanvasStore.setState((s) => ({
      nodes: s.nodes.map((n) =>
        n.id === id ? { ...n, position: { x: 0, y: 0 }, width: pane.width, height: pane.height } : n,
      ),
    }));
  }
}



// ---- Interactive popups ----------------------------------------------------

function ApprovalCard({
  approval,
  onResolve,
}: {
  approval: AgentApproval;
  onResolve: (id: string, approved: boolean, remember?: boolean) => void;
}) {
  return (
    <div className="mb-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 last:mb-0">
      <div className="mb-1 text-[11px] font-medium text-amber-200">
        Run this command?
      </div>
      <pre className="mb-2 max-h-24 overflow-auto whitespace-pre-wrap rounded bg-[var(--bg)] px-2 py-1 font-mono text-[11px] text-gray-300">
        {approval.command}
      </pre>
      <div className="flex flex-col gap-1.5">
        <button
          onClick={() => onResolve(approval.id, true, false)}
          className="rounded-md bg-blue-600 px-2.5 py-1 text-[11px] text-white hover:bg-blue-500"
        >
          Yes, run it
        </button>
        <button
          onClick={() => onResolve(approval.id, true, true)}
          className="rounded-md border border-blue-500/40 bg-blue-500/10 px-2.5 py-1 text-[11px] text-blue-200 hover:bg-blue-500/20"
        >
          Yes, and don't ask again this chat
        </button>
        <button
          onClick={() => onResolve(approval.id, false)}
          className="rounded-md border border-[var(--border)] px-2.5 py-1 text-[11px] text-gray-300 hover:bg-[var(--border)]"
        >
          No, don't run it
        </button>
      </div>
    </div>
  );
}

function QuestionCard({
  question,
  onAnswer,
}: {
  question: AgentQuestion;
  onAnswer: (id: string, answer: string) => void;
}) {
  const [picked, setPicked] = useState<Record<number, string[]>>({});
  const [other, setOther] = useState<Record<number, string>>({});

  const toggle = (qi: number, opt: string, multi?: boolean) =>
    setPicked((p) => {
      const cur = p[qi] ?? [];
      if (multi) {
        return {
          ...p,
          [qi]: cur.includes(opt) ? cur.filter((o) => o !== opt) : [...cur, opt],
        };
      }
      return { ...p, [qi]: cur.includes(opt) ? [] : [opt] };
    });

  const submit = () => {
    const parts = question.questions.map((q, qi) => {
      const chosen = [...(picked[qi] ?? [])];
      const free = (other[qi] ?? "").trim();
      if (free) chosen.push(free);
      return `Q: ${q.question}\nA: ${chosen.join(", ") || "(no answer)"}`;
    });
    onAnswer(question.id, parts.join("\n\n"));
  };

  return (
    <div className="mb-2 rounded-md border border-indigo-500/40 bg-indigo-500/10 p-2 last:mb-0">
      <div className="mb-1.5 text-[11px] font-medium text-indigo-200">
        The agent needs your input
      </div>
      {question.questions.map((q, qi) => (
        <div key={qi} className="mb-2 last:mb-0">
          {q.header && (
            <div className="text-[10px] uppercase tracking-wider text-indigo-300/70">
              {q.header}
            </div>
          )}
          <div className="mb-1 text-[12px] text-gray-200">{q.question}</div>
          {q.options && q.options.length > 0 && (
            <div className="mb-1 flex flex-wrap gap-1">
              {q.options.map((opt) => {
                const on = (picked[qi] ?? []).includes(opt);
                return (
                  <button
                    key={opt}
                    onClick={() => toggle(qi, opt, q.multi)}
                    className={`rounded-full border px-2 py-0.5 text-[10px] ${
                      on
                        ? "border-indigo-500 bg-indigo-600/40 text-indigo-100"
                        : "border-[var(--border)] text-gray-300 hover:bg-[var(--border)]"
                    }`}
                  >
                    {opt}
                  </button>
                );
              })}
            </div>
          )}
          <input
            value={other[qi] ?? ""}
            onChange={(e) => setOther((o) => ({ ...o, [qi]: e.target.value }))}
            placeholder="Other… (type your own answer)"
            className="w-full rounded border border-[var(--border-strong)] bg-[var(--bg)] px-2 py-1 text-[11px] text-gray-200 outline-none placeholder:text-gray-600 focus:border-[#3d4a61]"
          />
        </div>
      ))}
      <div className="flex justify-end">
        <button
          onClick={submit}
          className="rounded-md bg-indigo-600 px-2.5 py-1 text-[11px] text-white hover:bg-indigo-500"
        >
          Send answer
        </button>
      </div>
    </div>
  );
}

export const AgentNodeView = memo(function AgentNodeView({ id, selected }: NodeProps<AgentNodeType>) {
  const openSettings = useUiStore((s) => s.openSettings);

  // Node chrome: focus on click, drag by header (React Flow), tile counter-scale.
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);



  const {

    sessionId,

    messages,

    conversations,

    streamingSegments,

    streamStats,

    contextUsage,

    conversationCostUsd,

    streaming,

    queued,

    error,

    targets,

    pendingApprovals,

    pendingQuestions,

    activeIntakeGoalId,

    planMode,
    agentMode,

    send,
    enqueueOrSend,
    updateQueued,
    removeQueued,

    retryLast, clearError, setTargets,

    togglePlanMode,
    setAgentMode,
    stop,

    init,

    newConversation,

    openConversation,

    renameConversation,

    exportConversationMarkdown,

    resolveApproval,

    answerQuestion,

  } = useAgentStore();



  const maskHost = useMaskHost();
  const vpsList = useVpsStore((s) => s.vpsList);

  const loadVps = useVpsStore((s) => s.load);

  const loadSettings = useSettingsStore((s) => s.load);

  const providers = useSettingsStore((s) => s.providers);

  const activeProviderId = useSettingsStore((s) => s.settings["agent.active_provider"]);
  const activeModel = useSettingsStore((s) => s.settings["agent.active_model"]);
  const visionMode = parseVisionMode(useSettingsStore((s) => s.settings["agent.vision_mode"]));
  const visionProviderId = useSettingsStore((s) => s.settings["agent.vision_provider"]) ?? "";
  const visionModel = useSettingsStore((s) => s.settings["agent.vision_model"]) ?? "";

  // Reasoning effort capability control, persisted.
  const [reasoning, setReasoning] = useState<ReasoningLevel>(() => {
    const v = useSettingsStore.getState().settings["agent.reasoning_level"];
    return v === "low" || v === "medium" || v === "high" || v === "off" ? v : "off";
  });
  const setReasoningPersisted = useCallback((r: ReasoningLevel) => {
    setReasoning(r);
    void useSettingsStore.getState().set("agent.reasoning_level", r);
  }, []);

  const handleCycleSafety = useCallback(() => {
    const settings = useSettingsStore.getState().settings;
    const cur = settings["agent.safety_mode"] ?? "approve";
    const next = cur === "full" ? "allowlist" : cur === "allowlist" ? "approve" : "full";
    void useSettingsStore.getState().set("agent.safety_mode", next);
  }, []);

  const handleStop = useCallback(() => {
    setLoopTask(null);
    void stop();
  }, [stop]);

  const handlePickModel = useCallback(() => setPicker({ kind: "model" }), []);
  const handlePickContext = useCallback(() => setPicker({ kind: "ctx" }), []);
  const handlePickVision = useCallback(() => setPicker({ kind: "vision" }), []);
  const handlePickSafety = useCallback(() => setPicker({ kind: "safety" }), []);
  const handlePickReasoning = useCallback(() => setPicker({ kind: "reasoning" }), []);

  // Git pill: the repo the agent is working on (active workspace project).
  const activeWsId = useWorkspaceStore((s) => s.activeId);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const project = useMemo(() => {
    const ws = workspaces.find((w) => w.id === activeWsId);
    if (!ws?.project_json) return null;
    try {
      return JSON.parse(ws.project_json) as { kind: "local" | "vps"; path: string; vps_id?: string };
    } catch {
      return null;
    }
  }, [workspaces, activeWsId]);
  const gitInfo = useGitBranch({
    enabled: !!project?.path,
    path: project?.path,
    vpsId: project?.kind === "vps" ? project.vps_id : null,
  });
  const gitLabel = gitInfo ? `${gitInfo.branch}${gitInfo.dirty ? "*" : ""}` : null;

  /** Execute a code-block command: open/reuse a terminal for the target vps and
   *  auto-run (full perms / allowlisted) or type-and-wait (approve). */
  const executeCommand = useCallback((code: string) => {
    const canvas = useCanvasStore.getState();
    const vpsList = useVpsStore.getState().vpsList;
    const currentTargets = useAgentStore.getState().targets;
    // Resolve target: the agent's selected targets first, else the first vps.
    const targetId = currentTargets[0] ?? vpsList[0]?.id;
    const vps = vpsList.find((v) => v.id === targetId);
    if (!vps) {
      void notify("Execute", "No server selected — pick a target first (/targets).");
      return;
    }
    // Existing terminal for this vps? Reuse it (focus); else open a new one.
    let nodeId = canvas.nodes.find(
      (n) => n.type === "terminal" && String(n.data.vpsId) === vps.id,
    )?.id;
    if (!nodeId) {
      nodeId = canvas.addVps(vps);
    } else {
      canvas.focus(nodeId);
    }
    // Safety: full → run; allowlist → run if read-only; approve → type & wait.
    const settings = useSettingsStore.getState().settings;
    const perVps: Record<string, string> = {};
    for (const [k, v] of Object.entries(settings)) {
      if (k.startsWith("agent.safety_mode.")) perVps[k.slice("agent.safety_mode.".length)] = v;
    }
    const mode = effectiveMode(settings["agent.safety_mode"], vps.id, perVps);
    const send = shouldAutoRun(mode, code);
    canvas.queueTerminalCommand(nodeId, code, send);
    void notify(
      "Execute",
      send
        ? `Running on ${vps.name} (${mode})`
        : `Opened ${vps.name} — command typed, press Enter to run (${mode})`,
    );
  }, []);

  /** VPS context passed to code blocks so Execute can show the target name. */
  const executeTarget = useMemo(() => {
    // Use the reactive vpsList (not a getState() snapshot) so the Execute button
    // appears once servers finish loading, not only when targets change.
    const targetId = targets[0] ?? vpsList[0]?.id;
    const vps = vpsList.find((v) => v.id === targetId);
    return vps ? { name: vps.name, host: vps.host } : null;
  }, [targets, vpsList]);

  const sessionCache = useMemo(() => sessionCacheFromMessages(messages), [messages]);
  const lastAssistantStats = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].tokenStats) return messages[i].tokenStats!;
    }
    return null;
  }, [messages]);
  const displayStats = displayTurnStats(streamStats, lastAssistantStats);

  // Effective safety mode for the current target (global + per-VPS override),
  // so the permissions pill shows the truth, not just the global setting.
  const safetyModeGlobal = useSettingsStore((s) => s.settings["agent.safety_mode"]);
  const effectiveSafetyMode = useMemo(() => {
    const settings = useSettingsStore.getState().settings;
    const perVps: Record<string, string> = {};
    for (const [k, v] of Object.entries(settings)) {
      if (k.startsWith("agent.safety_mode.")) perVps[k.slice("agent.safety_mode.".length)] = v;
    }
    const targetId = targets[0] ?? vpsList[0]?.id;
    return effectiveMode(safetyModeGlobal, targetId ?? undefined, perVps);
  }, [targets, vpsList, safetyModeGlobal]);



  const [input, setInput] = useState("");
  const [copiedError, setCopiedError] = useState(false);
  const [intakeSpec, setIntakeSpec] = useState<GoalSpec | null>(null);
  const [intakeStatus, setIntakeStatus] = useState<string | null>(null);
  const [pendingImages, setPendingImages] = useState<ChatImage[]>([]);
  const [pendingSnippets, setPendingSnippets] = useState<ChatSnippet[]>([]);
  const [previewSnippet, setPreviewSnippet] = useState<ChatSnippet | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const agentRootRef = useRef<HTMLDivElement>(null);
  const pendingImagesRef = useRef(pendingImages);
  pendingImagesRef.current = pendingImages;
  const pendingSnippetsRef = useRef(pendingSnippets);
  pendingSnippetsRef.current = pendingSnippets;
  const askDraftRef = useRef("");
  const selectedRef = useRef(!!selected);
  selectedRef.current = !!selected;

  // Persist draft per conversation so switching sessions does not lose typed text.
  useEffect(() => {
    try {
      const key = `xconsole-agent-draft:${sessionId}`;
      const saved = localStorage.getItem(key);
      setInput(saved ?? "");
    } catch {
      setInput("");
    }
  }, [sessionId]);

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        const key = `xconsole-agent-draft:${sessionId}`;
        if (input) localStorage.setItem(key, input);
        else localStorage.removeItem(key);
      } catch {
        /* ignore */
      }
    }, 250);
    return () => clearTimeout(timer);
  }, [input, sessionId]);

  const history = useInputHistory(setInput);

  // Up/Down recalls previously sent user messages (shell-style). null = not recalling.
  const recallIdx = useRef<number | null>(null);
  // Mirrors the picker state so the Escape handler (declared before the state)
  // can see whether a picker is open.
  const pickerOpenRef = useRef(false);

  // Escape closes pickers, then stops the agent. It never closes the window —
  // that's only via the ✕ button or removing the node.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Don't steal Escape from dialogs/inputs that handle it (the picker input
      // handles its own Escape via onCancel).
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) {
        return;
      }
      e.preventDefault();
      // 1. A picker (model/targets/history/…) is open → close it first.
      if (pickerOpenRef.current) {
        setPicker(null);
        setPendingProviderId(null);
        return;
      }
      // 2. Streaming → stop the agent.
      if (useAgentStore.getState().streaming) {
        setLoopTask(null);
        void useAgentStore.getState().stop();
        return;
      }
      // 3. Otherwise: nothing — Escape does not close the window.
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);


  // Voice: spoken replies (TTS) + hands-free conversation.
  const ttsEnabled = useVoiceStore((s) => s.ttsEnabled);
  const [voiceError, setVoiceError] = useState("");
  const [voiceStatus, setVoiceStatus] = useState("");

  // Transcribe; if local whisper isn't installed yet, set it up automatically
  // (download binary + model the first time) and retry — no manual button needed.
  const transcribeAuto = async (wav: string): Promise<string> => {
    const vs = useVoiceStore.getState();
    try {
      return await api.transcribe(wav, vs.sttEngine, vs.sttModel || undefined, vs.sttLang);
    } catch (e) {
      const msg = String(e);
      const notReady = /not found|No whisper model|did not become ready/i.test(msg);
      if (vs.sttEngine !== "local" || !notReady) throw e;
      setVoiceStatus("Setting up local voice (first time, ~1 min)…");
      const model = await api.setupWhisper();
      useVoiceStore.getState().update({ sttModel: model });
      setVoiceStatus("");
      return await api.transcribe(wav, "local", model, vs.sttLang);
    }
  };

  const toggleSpeaker = () => {
    const on = !useVoiceStore.getState().ttsEnabled;
    useVoiceStore.getState().update({ ttsEnabled: on });
    if (!on) cancelSpeech();
  };

  // Hands-free conversation: listen continuously, transcribe each utterance,
  // send it, speak the reply, then keep listening — no press/unpress.
  const [conversation, setConversation] = useState(false);
  const convRef = useRef<Conversation | null>(null);
  const convBusyRef = useRef(false);

  const handleUtterance = async (wav: string) => {
    if (convBusyRef.current) return;
    convBusyRef.current = true;
    const vs = useVoiceStore.getState();
    vs.setTranscribing(true);
    try {
      const text = await transcribeAuto(wav);
      if (text.trim()) {
        // Hands-free voice: use the lightweight, low-latency conversation prompt.
        await send(text.trim(), {
          providerId: vs.conversationProvider || undefined,
          conversation: true,
        });
      }
    } catch (e) {
      setVoiceError(String(e));
    } finally {
      vs.setTranscribing(false);
      // Stay paused until the spoken reply finishes (shouldPause checks isSpeaking).
      convBusyRef.current = false;
    }
  };

  const toggleConversation = async () => {
    if (conversation) {
      convRef.current?.stop();
      convRef.current = null;
      convBusyRef.current = false;
      setConversation(false);
      return;
    }
    try {
      // In conversation mode replies are always spoken.
      useVoiceStore.getState().update({ ttsEnabled: true });
      convRef.current = await startConversation({
        onUtterance: (wav) => void handleUtterance(wav),
        // Keep listening even while the assistant is speaking so you can barge in.
        // Only pause while we're transcribing/sending a turn (avoids overlap).
        shouldPause: () => convBusyRef.current || useAgentStore.getState().streaming,
        // Barge-in: if you start talking while it's replying, cut it off.
        onSpeechStart: () => {
          if (isSpeaking()) cancelSpeech();
        },
      });
      setConversation(true);
      setVoiceError("");
    } catch {
      setVoiceError("Microphone access was blocked. Allow the mic for this app and try again.");
    }
  };

  // Tear down the mic if the panel unmounts mid-conversation.
  useEffect(() => {
    return () => {
      convRef.current?.stop();
      convRef.current = null;
    };
  }, []);

  type PickerKind =
    | "model"
    | "model-models"
    | "targets"
    | "history"
    | "ctx"
    | "cost"
    | "help"
    | "safety"
    | "reasoning"
    | "vision"
    | "vision-provider"
    | "vision-models"
    | "vision-ask"
    | "mode"
    | "prices";
  const [picker, setPicker] = useState<{ kind: PickerKind } | null>(null);
  const [showTrajectory, setShowTrajectory] = useState(false);
  const [modelPrices, setModelPrices] = useState<Record<string, { input: number; output: number; cache_read: number; cache_write: number }>>({});
  const [syncingPrices, setSyncingPrices] = useState(false);
  /** Provider id chosen in the first /model level — second level lists its models. */
  const [pendingProviderId, setPendingProviderId] = useState<string | null>(null);
  const pickerRef = useRef(picker);
  const pendingProviderRef = useRef(pendingProviderId);
  pickerRef.current = picker;
  pendingProviderRef.current = pendingProviderId;
  // Keep the Escape handler's ref in sync with the picker state.
  useEffect(() => {
    pickerOpenRef.current = picker !== null;
  }, [picker]);

  // /loop state: re-send the same task until the agent finishes or the user stops.
  const [loopTask, setLoopTask] = useState<string | null>(null);
  const [loopCount, setLoopCount] = useState(0);
  const loopMax = 10;
  const startLoop = (task: string) => {
    setLoopTask(task);
    setLoopCount(1);
    void send(task);
  };
  // When a loop turn completes (streaming stops), re-send unless stopped or capped.
  useEffect(() => {
    if (!loopTask || streaming) return;
    if (loopCount >= loopMax) {
      setLoopTask(null);
      return;
    }
    const t = setTimeout(() => {
      setLoopCount((c) => c + 1);
      void send(loopTask);
    }, 600);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [streaming, loopTask]);

  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Auto-grow the composer on any input change (typing, history recall, draft
  // restore, send) — not just while typing.
  useLayoutEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 132)}px`;
  }, [input]);

  // Refocus the composer when a turn finishes, so the user can type the next
  // message without clicking. Only steals focus when it isn't inside a dialog
  // or another input.
  useEffect(() => {
    if (streaming) return;
    const el = inputRef.current;
    if (!el) return;
    const active = document.activeElement;
    if (active && active !== document.body && !el.contains(active)) return;
    el.focus();
  }, [streaming]);

  // Agent console font size (A−/A+ in the status line), persisted like terminals.
  const [consoleFontSize, setConsoleFontSize] = useState<number>(() => {
    try {
      const n = Number(localStorage.getItem("xconsole-agent-font"));
      return n >= 9 && n <= 18 ? n : 11;
    } catch {
      return 11;
    }
  });
  const bumpFont = (delta: number) => {
    setConsoleFontSize((s) => {
      const next = Math.min(18, Math.max(9, s + delta));
      try {
        localStorage.setItem("xconsole-agent-font", String(next));
      } catch {
        /* ignore */
      }
      return next;
    });
  };



  useEffect(() => {

      loadVps();

      loadSettings();

      void init();

  }, [loadVps, loadSettings, init]);

  useEffect(() => {
    if (!activeIntakeGoalId) {
      setIntakeSpec(null);
      setIntakeStatus(null);
      return;
    }
    let alive = true;
    let un: (() => void) | undefined;
    const refresh = async () => {
      if (!activeIntakeGoalId) return;
      try {
        const s: GoalSession = await api.getGoal(activeIntakeGoalId);
        if (!alive) return;
        setIntakeStatus(s.status);
        setIntakeSpec(parseGoalSpec(s.spec_json));
      } catch {
        /* board / chat still usable */
      }
    };
    void refresh();
    void onGoalEvent(activeIntakeGoalId, () => {
      void refresh();
    }).then((fn) => {
      un = fn;
    });
    return () => {
      alive = false;
      un?.();
    };
  }, [activeIntakeGoalId]);



  const activeProvider = useMemo(
    () => providers.find((p) => p.id === activeProviderId) ?? providers[0],
    [providers, activeProviderId],
  );

  /** Options for the /model picker: enabled providers + their configured model. */
  const modelOptions = useMemo<CLIPickerOption[]>(() => {
    return providers
      .filter((p) => p.enabled)
      .map((p) => ({
        id: p.id,
        label: p.name || p.kind,
        detail: p.model || p.kind,
        selected: p.id === activeProvider?.id,
      }));
  }, [providers, activeProvider]);

  const targetOptions = useMemo<CLIPickerOption[]>(
    () =>
      vpsList.map((v) => ({
        id: v.id,
        label: v.name,
        detail: maskHost(v.host),
        selected: targets.includes(v.id),
      })),
    [vpsList, targets, maskHost],
  );

  const historyOptions = useMemo<CLIPickerOption[]>(
    () =>
      conversations.map((c) => {
        const timeStr = formatSessionDate(c.updated_at);
        const isCurrent = c.id === sessionId;
        const detailParts: string[] = [];
        if (isCurrent) detailParts.push("current");
        if (timeStr) detailParts.push(timeStr);
        return {
          id: c.id,
          label: c.title || `Session ${c.id.slice(0, 8)}`,
          detail: detailParts.join(" · ") || undefined,
          actionLabel: "✎ Rename",
          onAction: async (opt) => {
            const newTitle = await dialog.prompt({
              title: "Rename session",
              label: "Enter a new title for this session:",
              defaultValue: opt.label,
              confirmText: "Rename",
            });
            if (newTitle && newTitle.trim()) {
              await renameConversation(opt.id, newTitle.trim());
            }
          },
        };
      }),
    [conversations, sessionId, renameConversation],
  );

  const helpOptions = useMemo<CLIPickerOption[]>(
    () => [
      ...SLASH_COMMANDS.map((c) => ({
        id: c.syntax,
        label: c.syntax,
        detail: c.description,
      })),
      ...KEYBINDS.map((k) => ({
        id: k.keys,
        label: k.keys,
        detail: k.action,
      })),
    ],
    [],
  );

  const safetyOptions = useMemo<CLIPickerOption[]>(() => {
    const cur = effectiveSafetyMode;
    return [
      {
        id: "full",
        label: "Full permissions",
        detail: "Auto-run all commands without confirmation",
        selected: cur === "full",
      },
      {
        id: "allowlist",
        label: "Allowlist only",
        detail: "Auto-run safe read-only commands; ask for dangerous ones",
        selected: cur === "allowlist",
      },
      {
        id: "approve",
        label: "Ask every time",
        detail: "Prompt for confirmation on every command",
        selected: cur === "approve",
      },
  }, [effectiveSafetyMode]);

  const modeOptions = useMemo<CLIPickerOption[]>(() => [
    {
      id: "auto",
      label: "🤖 Auto (Smart Detection)",
      detail: "Auto-detects plan vs code vs minimal based on user prompt",
      selected: agentMode === "auto",
    },
    {
      id: "standard",
      label: "🌐 Standard",
      detail: "Full capabilities, DevOps tools & general copilot guidance",
      selected: agentMode === "standard",
    },
    {
      id: "code",
      label: "⚡ Code",
      detail: "Focused coding, test writing, refactoring & file implementation",
      selected: agentMode === "code",
    },
    {
      id: "plan",
      label: "📋 Plan",
      detail: "Safe read-only investigation, requires plan approval before mutations",
      selected: agentMode === "plan",
    },
    {
      id: "minimal",
      label: "🛡️ Minimal",
      detail: "Compact token-efficient prompt with lightweight context",
      selected: agentMode === "minimal",
    },
  ], [agentMode]);

  const reasoningOptions = useMemo<CLIPickerOption[]>(() => [
    {
      id: "off",
      label: "Off",
      detail: "Standard fast generation (no thinking budget)",
      selected: reasoning === "off",
    },
    {
      id: "low",
      label: "Low",
      detail: "Light reasoning effort (brief thinking)",
      selected: reasoning === "low",
    },
    {
      id: "medium",
      label: "Medium",
      detail: "Balanced reasoning effort (standard thinking)",
      selected: reasoning === "medium",
    },
    {
      id: "high",
      label: "High",
      detail: "Maximum reasoning effort (deep thinking)",
      selected: reasoning === "high",
    },
  ], [reasoning]);

  const visionProvider = useMemo(
    () => providers.find((p) => p.id === visionProviderId) ?? providers.find(isGeminiProvider),
    [providers, visionProviderId],
  );
  const visionModelLabel = visionModel || (visionProvider ? defaultVisionModel(visionProvider) : "");
  const visionOptions = useMemo<CLIPickerOption[]>(
    () => [
      {
        id: "ask",
        label: "Ask before sending images",
        detail: "Command Code default",
        selected: visionMode === "ask",
      },
      {
        id: "enabled",
        label: "Always send images",
        detail: "Native if the session model can see, else the vision model",
        selected: visionMode === "enabled",
      },
      {
        id: "disabled",
        label: "Don't send images",
        detail: "Keep [Image #n] text only",
        selected: visionMode === "disabled",
      },
      {
        id: "__model__",
        label: "Choose vision model…",
        detail: visionProvider
          ? `${visionProvider.name} · ${visionModelLabel || "default"}`
          : "Auto — Gemini if a key is configured",
      },
    ],
    [visionMode, visionProvider, visionModelLabel],
  );
  const visionProviderOptions = useMemo<CLIPickerOption[]>(() => {
    const enabled = providers.filter((p) => p.enabled);
    const gemini = enabled.filter(isGeminiProvider);
    const rest = enabled.filter((p) => !isGeminiProvider(p));
    return [...gemini, ...rest].map((p) => ({
      id: p.id,
      label: p.name || p.kind,
      detail: isGeminiProvider(p)
        ? `recommended · ${defaultVisionModel(p, p.id === visionProviderId ? visionModel : undefined)}`
        : p.model || p.kind,
      selected: p.id === (visionProviderId || visionProvider?.id),
    }));
  }, [providers, visionProviderId, visionModel, visionProvider]);
  const visionModelOptions = useMemo<CLIPickerOption[]>(() => {
    const p = providers.find((x) => x.id === pendingProviderId);
    if (!p) return [];
    const catalog = catalogForProvider(p);
    const ids = new Set<string>();
    const opts: CLIPickerOption[] = [];
    const add = (id: string, detail: string) => {
      if (!id || ids.has(id)) return;
      ids.add(id);
      opts.push({ id, label: id, detail, selected: id === visionModel });
    };
    if (isGeminiProvider(p)) add("gemini-2.5-flash", "recommended");
    add(p.model || "", "configured");
    for (const m of catalog?.models ?? []) add(m, "catalog");
    return opts;
  }, [providers, pendingProviderId, visionModel]);
  const visionAskOptions = useMemo<CLIPickerOption[]>(
    () => [
      { id: "once", label: "Send with vision", detail: "This message only" },
      { id: "enable", label: "Always send images", detail: "Remember in settings" },
      { id: "skip", label: "Send without pixels", detail: "Keep [Image #n] text only" },
    ],
    [],
  );

  const [providerDynamicModels, setProviderDynamicModels] = useState<string[]>([]);
  useEffect(() => {
    if (picker?.kind !== "model-models" || !pendingProviderId) {
      setProviderDynamicModels([]);
      return;
    }
    const p = providers.find((x) => x.id === pendingProviderId);
    if (!p) {
      setProviderDynamicModels([]);
      return;
    }
    const cli =
      p.kind === "opencode_cli" ||
      p.kind === "antigravity_cli" ||
      p.kind === "cursor" ||
      p.kind === "codex_cli";

    let alive = true;
    if (cli) {
      api
        .aiCliModels(p.id)
        .then((list) => {
          if (alive) setProviderDynamicModels(list);
        })
        .catch(() => {
          if (alive) setProviderDynamicModels([]);
        });
    } else if (p.base_url && (p.kind === "openai" || p.kind === "anthropic")) {
      const catalog = catalogForProvider(p);
      const flavor = catalog?.flavor || (p.kind === "anthropic" ? "anthropic" : "openai");
      api
        .listModels(flavor, p.base_url, "")
        .then((list) => {
          if (alive && list.length > 0) setProviderDynamicModels(list);
        })
        .catch(() => {
          if (alive) setProviderDynamicModels([]);
        });
    } else {
      setProviderDynamicModels([]);
    }
    return () => {
      alive = false;
    };
  }, [picker?.kind, pendingProviderId, providers]);

  /** Options for the /model picker's SECOND level: models of the chosen provider. */
  const providerModelOptions = useMemo<CLIPickerOption[]>(() => {
    const p = providers.find((x) => x.id === pendingProviderId);
    if (!p) return [];
    const catalog = catalogForProvider(p);
    const ids = new Set<string>();
    const opts: CLIPickerOption[] = [];
    const add = (id: string, detail: string) => {
      if (!id || ids.has(id)) return;
      ids.add(id);
      opts.push({
        id,
        label: id,
        detail,
        selected: id === activeModel || id === p.model,
      });
    };
    add(p.model || "", "configured");
    for (const m of providerDynamicModels) add(m, "api / live");
    for (const m of catalog?.models ?? []) add(m, "catalog");
    return opts;
  }, [providers, pendingProviderId, activeModel, providerDynamicModels]);

  /** Handle a picker selection. */
  const onPickerPick = (opt: CLIPickerOption) => {
    const kind = pickerRef.current?.kind;
    if (!kind) return;
    switch (kind) {
      case "model": {
        // First level: provider chosen → second level lists its models.
        setPendingProviderId(opt.id);
        setPicker({ kind: "model-models" });
        break;
      }
      case "model-models": {
        // Second level: model chosen → set provider + model.
        void useSettingsStore.getState().set("agent.active_provider", pendingProviderRef.current ?? "");
        void useSettingsStore.getState().set("agent.active_model", opt.id);
        setPendingProviderId(null);
        setPicker(null);
        break;
      }
      case "targets": {
        const ids = opt.id === "__done__" ? undefined : opt.id;
        if (ids !== undefined) {
          const next = targets.includes(ids)
            ? targets.filter((t) => t !== ids)
            : [...targets, ids];
          setTargets(next);
          return; // keep picker open for multi-select
        }
        setPicker(null);
        break;
      }
      case "history":
        if (opt.id !== sessionId) void openConversation(opt.id);
        setPicker(null);
        break;
      case "safety": {
        if (opt.id === "full" || opt.id === "allowlist" || opt.id === "approve") {
          void useSettingsStore.getState().set("agent.safety_mode", opt.id);
        }
        setPicker(null);
        break;
      }
      case "reasoning": {
        if (opt.id === "off" || opt.id === "low" || opt.id === "medium" || opt.id === "high") {
          setReasoningPersisted(opt.id);
        }
        setPicker(null);
        break;
      }
      case "vision": {
        if (opt.id === "__model__") {
          setPicker({ kind: "vision-provider" });
          break;
        }
        void useSettingsStore.getState().set("agent.vision_mode", opt.id);
        setPicker(null);
        break;
      }
      case "vision-provider": {
        void useSettingsStore.getState().set("agent.vision_provider", opt.id);
        const p = providers.find((x) => x.id === opt.id);
        if (p) {
          void useSettingsStore.getState().set("agent.vision_model", defaultVisionModel(p));
        }
        setPendingProviderId(opt.id);
        setPicker({ kind: "vision-models" });
        break;
      }
      case "vision-models": {
        void useSettingsStore.getState().set("agent.vision_model", opt.id);
        setPendingProviderId(null);
        setPicker(null);
        break;
      }
      case "vision-ask": {
        const mode = opt.id;
        if (mode === "enable") {
          void useSettingsStore.getState().set("agent.vision_mode", "enabled");
        }
        const sendImages = mode !== "skip";
        const text = askDraftRef.current;
        const imgs = sendImages ? pendingImagesRef.current : undefined;
        setInput("");
        history.reset("");
        setPendingImages([]);
        setPicker(null);
        enqueueOrSend(text, imgs);
        break;
      }
      case "mode": {
        if (
          opt.id === "auto" ||
          opt.id === "standard" ||
          opt.id === "code" ||
          opt.id === "plan" ||
          opt.id === "minimal"
        ) {
          setAgentMode(opt.id);
        }
        setPicker(null);
        break;
      }
      default:
        setPicker(null);
    }
  };



  const [slashIndex, setSlashIndex] = useState(0);
  const slashSuggestions = useMemo(() => {
    return isSlashInput(input) ? filterSlashCommands(input) : [];
  }, [input]);

  const executeSlashAction = async (cmd: SlashCommandDef) => {
    setInput("");
    history.reset("");
    if (cmd.actionKey === "new") {
      await newConversation();
    } else if (cmd.actionKey === "clear") {
      setInput("");
      history.reset("");
    } else if (cmd.actionKey === "history") {
      setPicker({ kind: "history" });
    } else if (cmd.actionKey === "rename") {
      const cur = conversations.find((c) => c.id === sessionId);
      const newTitle = await dialog.prompt({
        title: "Rename session",
        label: "Enter a new title for this session:",
        defaultValue: cur?.title || "",
        confirmText: "Rename",
      });
      if (newTitle && newTitle.trim()) {
        await renameConversation(sessionId, newTitle.trim());
      }
    } else if (cmd.actionKey === "model") {
      setPicker({ kind: "model" });
    } else if (cmd.actionKey === "targets") {
      setPicker({ kind: "targets" });
    } else if (cmd.actionKey === "safety") {
      setPicker({ kind: "safety" });
    } else if (cmd.actionKey === "reasoning") {
      setPicker({ kind: "reasoning" });
    } else if (cmd.actionKey === "plan") {
      togglePlanMode();
    } else if (cmd.actionKey === "export") {
      const md = exportConversationMarkdown();
      void navigator.clipboard.writeText(md);
      void notify("Conversation exported", "Markdown copied to clipboard");
    } else if (cmd.actionKey === "compact") {
      void send("Please summarize our progress and key context so far, compacting the conversation history.");
    } else if (cmd.actionKey === "ctx") {
      setPicker({ kind: "ctx" });
    } else if (cmd.actionKey === "cost") {
      setPicker({ kind: "cost" });
    } else if (cmd.actionKey === "voice") {
      toggleSpeaker();
    } else if (cmd.actionKey === "conversation") {
      void toggleConversation();
    } else if (cmd.actionKey === "help") {
      setPicker({ kind: "help" });
    } else if (cmd.actionKey === "vision") {
      setPicker({ kind: "vision" });
    } else if (cmd.actionKey === "close") {
      removeNode(id);
    } else if (cmd.actionKey === "prices") {
      void api.aiGetModelPrices().then((p) => setModelPrices(p)).catch(() => {});
      setPicker({ kind: "prices" });
    } else if (cmd.actionKey === "mode") {
      setPicker({ kind: "mode" });
    } else if (cmd.actionKey === "trajectory") {
      setShowTrajectory(true);
    } else if (cmd.actionKey === "goal" || cmd.actionKey === "loop") {
      const prefix = `/${cmd.name} `;
      setInput(prefix);
      history.record(prefix);
    }
  };

  const pickSlashCommand = (cmd: SlashCommandDef) => {
    if (cmd.needsArg) {
      const prefix = `/${cmd.name} `;
      setInput(prefix);
      history.record(prefix);
      return;
    }
    void executeSlashAction(cmd);
  };

  const addImages = (imgs: ChatImage[]) => {
    if (imgs.length === 0) return;
    setPendingImages((cur) => [...cur, ...imgs].slice(0, 8));
    requestAnimationFrame(() => inputRef.current?.focus());
  };

  const insertComposerText = (text: string) => {
    const el = inputRef.current;
    const cur = el?.value ?? "";
    const start = el?.selectionStart ?? cur.length;
    const end = el?.selectionEnd ?? cur.length;
    const next = cur.slice(0, start) + text + cur.slice(end);
    setInput(next);
    history.record(next);
    requestAnimationFrame(() => {
      const box = inputRef.current;
      if (!box) return;
      const pos = start + text.length;
      box.focus();
      box.setSelectionRange(pos, pos);
    });
  };

  const attachClipboardImages = async (data: DataTransfer | null | undefined): Promise<boolean> => {
    const files = imagesFromClipboardEvent(data);
    if (files.length) {
      addImages(await Promise.all(files.map((f) => fileToChatImage(f))));
      return true;
    }
    const png = await clipboardImagePng();
    if (!png) return false;
    addImages([await bytesToChatImage(png, "clipboard.png")]);
    return true;
  };

  const onAgentPaste = (e: ClipboardEvent) => {
    const root = agentRootRef.current;
    const target = e.target as HTMLElement | null;
    const inside = !!(root && target && root.contains(target));
    if (!inside && !selectedRef.current) return;
    if (!inside && selectedRef.current) {
      if (target?.closest(".xterm, [data-terminal]")) return;
      if (
        target &&
        (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) &&
        !root?.contains(target)
      ) {
        return;
      }
    }

    const data = e.clipboardData;
    const htmlImages = imagesFromClipboardEvent(data);
    const looksImage = clipboardLooksLikeImage(data);
    const otherField =
      !!target &&
      target !== inputRef.current &&
      (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || !!target.isContentEditable);

    if (otherField && htmlImages.length === 0 && !looksImage) return;

    const text = data?.getData("text/plain") ?? "";
    if (!htmlImages.length && !looksImage && text && otherField) return;

    e.preventDefault();
    e.stopPropagation();
    void (async () => {
      const got = await attachClipboardImages(data);
      if (got) return;
      if (text && !otherField) {
        if (shouldCreateSnippet(text)) {
          const snip = createChatSnippet(text);
          setPendingSnippets((cur) => [...cur, snip].slice(0, 10));
          requestAnimationFrame(() => inputRef.current?.focus());
        } else {
          insertComposerText(text);
        }
      }
    })();
  };

  const onAgentPasteRef = useRef(onAgentPaste);
  onAgentPasteRef.current = onAgentPaste;

  useEffect(() => {
    const fn = (e: ClipboardEvent) => onAgentPasteRef.current(e);
    window.addEventListener("paste", fn, true);
    return () => window.removeEventListener("paste", fn, true);
  }, []);

  const loadImagePath = async (path: string): Promise<ChatImage | null> => {
    try {
      const b64 = await api.localFsReadBytes(path, 10 * 1024 * 1024);
      const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      return await bytesToChatImage(bin, fileBaseName(path));
    } catch (e) {
      void notify("Vision", String(e));
      return null;
    }
  };

  const attachPaths = async (paths: string[]) => {
    const imgs: ChatImage[] = [];
    for (const path of paths) {
      if (!isImagePath(path)) continue;
      const img = await loadImagePath(path);
      if (img) imgs.push(img);
    }
    if (imgs.length) addImages(imgs);
    else if (paths.length) void notify("Vision", "Drop a PNG, JPEG, GIF, or WebP.");
  };

  useEffect(() => {
    return onOsFilesDropped((target, paths) => {
      if (target !== "agent-composer") return;
      void attachPaths(paths);
    });
  }, []);

  const submit = () => {
    const trimmed = input.trim();
    const visionPath = trimmed.match(/^\/vision(?:\s+(.+))?$/i);
    if (visionPath && visionPath[1] && isImagePath(visionPath[1])) {
      setInput("");
      history.reset("");
      void attachPaths([visionPath[1].trim()]);
      return;
    }
    if (!trimmed && pendingImages.length === 0 && pendingSnippets.length === 0) return;
    // /rename [title] — rename current session
    const renameMatch = trimmed.match(/^\/rename(?:\s+(.+))?$/i);
    if (renameMatch) {
      const newTitle = renameMatch[1]?.trim();
      setInput("");
      history.reset("");
      recallIdx.current = null;
      if (newTitle) {
        void renameConversation(sessionId, newTitle);
      } else {
        const cur = conversations.find((c) => c.id === sessionId);
        void dialog
          .prompt({
            title: "Rename session",
            label: "Enter a new title for this session:",
            defaultValue: cur?.title || "",
            confirmText: "Rename",
          })
          .then((title) => {
            if (title && title.trim()) {
              void renameConversation(sessionId, title.trim());
            }
          });
      }
      return;
    }
    // /goal <objective> — start an autonomous goal session + open its kanban board.
    const goalMatch = trimmed.match(/^\/goal(?:\s+(.+))?$/i);
    if (goalMatch) {
      const objective = goalMatch[1]?.trim();
      if (!objective) {
        void notify("Goal", "Usage: /goal <objective> — e.g. /goal rank my site #1 for 'vps ssh manager'");
        return;
      }
      setInput("");
      history.reset("");
      recallIdx.current = null;
      void useGoalStore
        .getState()
        .start(objective)
        .then((goalId) => {
          useAgentStore.getState().setActiveIntakeGoal(goalId);
          useCanvasStore.getState().addGoal(goalId);
          const pane = useCanvasStore.getState().paneSize;
          const agent = useCanvasStore.getState().nodes.find((n) => n.type === "agent");
          if (agent && pane) {
            const w = Number(agent.width) || 0;
            const h = Number(agent.height) || 0;
            if (w >= pane.width - 4 && h >= pane.height - 4) {
              toggleAgentFillPane(agent.id);
            }
          }
          void notify("Goal", "Intake started — answer the agent's questions, then lock the goal.");
          void send(
            `Start intake for this autonomous goal.\n\nObjective: ${objective}\n\nAsk only what you need (ask_user), then call goal_propose_spec with a concrete spec (objective, success criteria, how you will check, hard constraints). Do not start the work until the user locks the goal on the board.`,
            { goalId },
          );
        })
        .catch((e) => notify("Goal", String(e)));
      return;
    }
    // /loop <task> — loop until the agent finishes (Esc to stop).
    const loopMatch = trimmed.match(/^\/loop(?:\s+(.+))?$/i);
    if (loopMatch) {
      const task = loopMatch[1]?.trim();
      if (!task) {
        // Bare /loop: re-loop the last user message.
        const lastUser = messages.filter((m) => m.role === "user").pop()?.content;
        if (!lastUser) return;
        startLoop(lastUser);
      } else {
        startLoop(task);
      }
      setInput("");
      history.reset("");
      recallIdx.current = null;
      return;
    }
    const exact = parseExactSlashCommand(trimmed);
    if (exact) {
      void executeSlashAction(exact);
      return;
    }
    const imgs = pendingImages.length > 0 ? pendingImages : undefined;
    if (imgs && visionMode === "ask") {
      askDraftRef.current = input;
      setPicker({ kind: "vision-ask" });
      return;
    }

    let promptText = trimmed;
    if (pendingSnippets.length > 0) {
      const snippetBlocks = pendingSnippets
        .map((s) => {
          const langTag = s.language === "text" ? "" : s.language;
          return `\`\`\`${langTag}\n${s.content}\n\`\`\``;
        })
        .join("\n\n");

      if (promptText) {
        promptText = `${promptText}\n\n${snippetBlocks}`;
      } else {
        promptText = snippetBlocks;
      }
    }

    enqueueOrSend(promptText, visionMode === "disabled" ? undefined : imgs);
    setInput("");
    history.reset("");
    setPendingImages([]);
    setPendingSnippets([]);
    recallIdx.current = null;
  };

  const canvasNodes = useCanvasStore((s) => s.nodes);
  const canvasVpsIds = useMemo(() => {
    const ids = new Set<string>();
    for (const n of canvasNodes) {
      const v = String(n.data.vpsId ?? "");
      if (v) ids.add(v);
    }
    return [...ids];
  }, [canvasNodes]);

  // If no targets picked yet but the canvas has hosts open, pre-select those.
  useEffect(() => {
    if (targets.length > 0 || canvasVpsIds.length === 0) return;
    setTargets(canvasVpsIds);
  }, [canvasVpsIds.join("|")]); // eslint-disable-line react-hooks/exhaustive-deps

  const activeTargetsInfo = useMemo(
    () =>
      vpsList
        .filter((v) => targets.includes(v.id))
        .map((v) => ({ id: v.id, name: v.name, host: maskHost(v.host) })),
    [vpsList, targets, maskHost],
  );

  const handlePickTargets = useCallback(() => {
    setPicker((cur) => (cur?.kind === "targets" ? null : { kind: "targets" }));
  }, []);



  return (
    <div
      ref={agentRootRef}
      data-agent-root
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-blue-500" : "border-[var(--border)]"}`}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
      onMouseDown={(e) => {
        const t = e.target as HTMLElement;
        if (t.closest("button, input, textarea, select, [data-picker]")) return;
        focus(id);
      }}
    >
      <NodeResizer
        minWidth={200}
        minHeight={240}
        isVisible
        lineClassName="!border-blue-500"
        handleClassName="!bg-blue-500"
      />

      {/* Slim terminal status line (no buttons — everything is a command). */}
      <div className="flex cursor-move select-none items-center gap-1 border-b border-[var(--border)] bg-[var(--surface)] px-2 py-1 font-mono text-[11px]">
        <span className="shrink-0 text-cyan-400">{streaming ? "●" : "❯"}</span>
        <span
          className="xc-agent-title max-w-[280px] truncate font-medium text-[var(--text)] hover:text-cyan-300"
          data-tooltip={`Session: ${conversations.find((c) => c.id === sessionId)?.title || "agent"} (double-click to rename)`}
          onDoubleClick={async (e) => {
            e.stopPropagation();
            const cur = conversations.find((c) => c.id === sessionId);
            const newTitle = await dialog.prompt({
              title: "Rename session",
              label: "Enter a new title for this session:",
              defaultValue: cur?.title || "",
              confirmText: "Rename",
            });
            if (newTitle && newTitle.trim()) {
              await renameConversation(sessionId, newTitle.trim());
            }
          }}
        >
          {conversations.find((c) => c.id === sessionId)?.title || "agent"}
        </span>
        {planMode ? (
          <span className="rounded bg-indigo-500/20 px-1 text-[9px] text-indigo-300">plan</span>
        ) : null}
        {loopTask ? (
          <span className="rounded bg-cyan-500/20 px-1 text-[9px] text-cyan-300">
            ⟳ {loopCount}/{loopMax}
          </span>
        ) : null}
        {ttsEnabled ? <span className="text-[9px] text-[var(--text-faint)]">🔊</span> : null}
        <span className="ml-auto flex items-center gap-0.5">
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => setShowTrajectory(true)}
            data-tooltip="Inspect trajectory & events (/trajectory)"
            className="rounded px-1.5 py-0.5 text-[10px] text-cyan-400 hover:bg-[var(--border)] hover:text-cyan-200 font-mono"
          >
            ⚡ trace
          </button>
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => bumpFont(-1)}
            data-tooltip="Smaller font"
            className="rounded px-1 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            A−
          </button>
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => bumpFont(1)}
            data-tooltip="Larger font"
            className="rounded px-1 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            A+
          </button>
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => removeNode(id)}
            data-tooltip="Close agent window"
            className="rounded px-1.5 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            ✕
          </button>
        </span>
      </div>

      {/* Body: nodrag so only the header starts a node drag (like TerminalNode) —
          text selection inside the console/composer works normally. */}
      <div
        className="nodrag flex min-h-0 flex-1 flex-col"
        onMouseDown={(e) => {
          const t = e.target as HTMLElement;
          if (t.closest("button, input, textarea, select, [data-picker], a")) return;
          inputRef.current?.focus();
        }}
      >
      {/* Messages */}
      {messages.length === 0 && !streaming ? (
        <div className="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-xs text-gray-600">
          <div className="space-y-2 font-mono">
            <p className="text-[var(--text-dim)]">agent@xconsole:~$</p>
            <p className="text-[10px] text-gray-700">
              Type a task, or /help for commands. /model picks the provider · /vision
              picks the image model · /targets selects hosts · Shift+Tab toggles plan mode.
            </p>
          </div>
        </div>
      ) : (
        <AgentConsole
          messages={messages}
          streamingSegments={streaming ? streamingSegments : []}
          streaming={streaming}
          expanded
          executeTarget={executeTarget}
          onExecute={executeCommand}
          fontSize={consoleFontSize}
        />
      )}

      {error && (
        <div className="flex items-start gap-2 border-t border-red-900/40 bg-red-950/20 px-3 py-2 text-xs text-red-400">
          <span className="nowheel min-w-0 flex-1 cursor-text select-text break-words font-mono text-[11px] leading-relaxed">
            {error}
          </span>
          <button
            type="button"
            className="shrink-0 rounded border border-red-800/50 bg-red-950/40 px-1.5 py-0.5 text-[10px] text-red-200 transition hover:bg-red-900/50"
            onClick={() => {
              void navigator.clipboard.writeText(error);
              setCopiedError(true);
              setTimeout(() => setCopiedError(false), 2000);
            }}
            data-tooltip="Copy error to clipboard"
          >
            {copiedError ? "Copied!" : "Copy"}
          </button>
          <button
            type="button"
            className="shrink-0 rounded border border-red-800/50 bg-red-950/40 px-1.5 py-0.5 text-[10px] text-red-200 transition hover:bg-red-900/50"
            onClick={() => void retryLast()}
            data-tooltip="Retry and continue from where it stopped"
          >
            Retry
          </button>
          <button
            type="button"
            className="shrink-0 rounded px-1 text-[10px] text-red-400/70 hover:text-red-200"
            onClick={() => clearError()}
            data-tooltip="Dismiss"
          >
            ✕
          </button>
        </div>
      )}

      {/* Interactive prompts: approvals, questions (plans open in the PlanModal) */}
      {(pendingApprovals.length > 0 ||
        pendingQuestions.length > 0 ||
        (activeIntakeGoalId && (!intakeStatus || intakeStatus === "intake"))) && (
        <div className="border-t border-[var(--border)] bg-[var(--bg)] px-3 py-2">
          {activeIntakeGoalId && (!intakeStatus || intakeStatus === "intake") && (
            <div className="mb-2 last:mb-0">
              <GoalLockCard
                spec={intakeSpec}
                onLock={() => {
                  void useGoalStore
                    .getState()
                    .confirm(activeIntakeGoalId, useAgentStore.getState().targets)
                    .then(() => useAgentStore.getState().setActiveIntakeGoal(null))
                    .catch((e) => notify("Goal", String(e)));
                }}
                onCancel={() => {
                  void useGoalStore
                    .getState()
                    .stop(activeIntakeGoalId)
                    .then(() => useAgentStore.getState().setActiveIntakeGoal(null))
                    .catch((e) => notify("Goal", String(e)));
                }}
              />
            </div>
          )}
          {pendingApprovals.map((a) => (
            <ApprovalCard key={a.id} approval={a} onResolve={resolveApproval} />
          ))}
          {pendingQuestions.map((q) => (
            <QuestionCard key={q.id} question={q} onAnswer={answerQuestion} />
          ))}
        </div>
      )}

      {/* In-console picker (CLI style) */}
      {picker && (
        <div
          data-picker
          className="nodrag nopan nowheel border-t border-[var(--border)] px-3 pb-2 pt-2"
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {picker.kind === "model" && (
            <CLIPicker
              title="Model — provider"
              options={modelOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter providers…"
            />
          )}
          {picker.kind === "model-models" && (
            <CLIPicker
              title="Model — choose model"
              options={providerModelOptions}
              onPick={onPickerPick}
              onCancel={() => {
                setPendingProviderId(null);
                setPicker(null);
              }}
              placeholder="Filter models…"
            />
          )}
          {picker.kind === "targets" && (
            <CLIPicker
              title="Targets"
              options={targetOptions}
              multi
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter hosts…"
            />
          )}
          {picker.kind === "history" && (
            <CLIPicker
              title="History"
              options={historyOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter conversations…"
            />
          )}
          {picker.kind === "ctx" && (
            <div className="rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-3 py-2 font-mono text-[11px]">
              <div className="mb-1 flex items-center justify-between">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
                  Context
                </span>
                <button
                  type="button"
                  onClick={() => setPicker(null)}
                  className="flex h-4 w-4 items-center justify-center rounded text-gray-500 hover:bg-[var(--border)] hover:text-white"
                  title="Close (Esc)"
                >
                  ✕
                </button>
              </div>
              {contextUsage ? (
                <>
                  <div className="mb-1 text-[var(--text)]">
                    {contextUsage.percent}% of {contextUsage.context_limit.toLocaleString()} tokens
                  </div>
                  {contextUsage.segments.map((s) => (
                    <div key={s.key} className="flex justify-between gap-3 text-[var(--text-faint)]">
                      <span>{s.label}</span>
                      <span>{s.tokens.toLocaleString()}</span>
                    </div>
                  ))}
                </>
              ) : (
                <div className="text-[var(--text-faint)]">No context usage yet.</div>
              )}
              <button
                type="button"
                onClick={() => setPicker(null)}
                className="mt-2 w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-[10px] text-[var(--text-dim)] hover:text-[var(--text)]"
              >
                Close
              </button>
            </div>
          )}
          {picker.kind === "cost" && (
            <div className="rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-3 py-2 font-mono text-[11px]">
              <div className="mb-1 flex items-center justify-between">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
                  Cost
                </span>
                <button
                  type="button"
                  onClick={() => setPicker(null)}
                  className="flex h-4 w-4 items-center justify-center rounded text-gray-500 hover:bg-[var(--border)] hover:text-white"
                  title="Close (Esc)"
                >
                  ✕
                </button>
              </div>
              <div className="text-[var(--text)]">
                This conversation: ${conversationCostUsd.toFixed(4)}
              </div>
              {sessionCache.turns > 0 ? (
                <div className="text-[var(--text-faint)]">{formatSessionCache(sessionCache)}</div>
              ) : null}
              <div className="text-[var(--text-faint)]">
                {displayStats?.promptTokens
                  ? `last turn: ${displayStats.promptTokens.toLocaleString()} in · ${displayStats.completionTokens.toLocaleString()} out`
                  : "No provider usage yet."}
              </div>
              <button
                type="button"
                onClick={() => setPicker(null)}
                className="mt-2 w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-[10px] text-[var(--text-dim)] hover:text-[var(--text)]"
              >
                Close
              </button>
            </div>
          )}
          {picker.kind === "help" && (
            <CLIPicker
              title="Help — commands & keybinds"
              options={helpOptions}
              onPick={() => setPicker(null)}
              onCancel={() => setPicker(null)}
            />
          )}
          {picker.kind === "safety" && (
            <CLIPicker
              title="Safety — Permissions"
              options={safetyOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter safety mode…"
            />
          )}
          {picker.kind === "reasoning" && (
            <CLIPicker
              title="Reasoning — Thinking Effort"
              options={reasoningOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter reasoning level…"
            />
          )}
          {picker.kind === "vision" && (
            <CLIPicker
              title="Vision"
              options={visionOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter…"
            />
          )}
          {picker.kind === "vision-provider" && (
            <CLIPicker
              title="Vision — provider (Gemini recommended)"
              options={visionProviderOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter providers…"
            />
          )}
          {picker.kind === "mode" && (
            <CLIPicker
              title="Agent Runtime Mode (DeepSeek Harness style)"
              options={[
                { id: "standard", label: "🌐 standard", detail: "Full autonomous toolset (shell, files, web, db, preview, goals)" },
                { id: "code", label: "⚡ code", detail: "Focused code editing, fast diffs, compiler checks" },
                { id: "plan", label: "📋 plan", detail: "Interactive architectural planning before executing changes" },
                { id: "minimal", label: "🛡️ minimal", detail: "Lightweight, minimal prompt with shell and file editing" },
              ]}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
              placeholder="Filter runtime mode…"
            />
          )}
          {picker.kind === "prices" && (
            <div className="rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-3 py-2 font-mono text-[11px]">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-cyan-400">
                  Model Pricing Catalog
                </span>
                <button
                  type="button"
                  onClick={() => setPicker(null)}
                  className="flex h-4 w-4 items-center justify-center rounded text-gray-500 hover:bg-[var(--border)] hover:text-white"
                  title="Close (Esc)"
                >
                  ✕
                </button>
              </div>
              <div className="mb-2 text-[10.5px] text-[var(--text-dim)]">
                Live rates in USD per 1M tokens (input / output / cache read):
              </div>
              <div className="max-h-44 overflow-y-auto space-y-1 pr-1 text-[10px]">
                {Object.entries(modelPrices).length > 0 ? (
                  Object.entries(modelPrices).slice(0, 60).map(([k, v]) => (
                    <div key={k} className="flex items-center justify-between border-b border-[var(--border)]/40 pb-0.5">
                      <span className="truncate max-w-[200px] text-gray-200">{k}</span>
                      <span className="text-gray-400 font-mono">
                        ${v.input.toFixed(2)} in · ${v.output.toFixed(2)} out · ${v.cache_read.toFixed(3)} r
                      </span>
                    </div>
                  ))
                ) : (
                  <div className="text-[var(--text-faint)]">
                    DeepSeek V4 Flash / Command Code: $0.14/M in · $0.28/M out · $0.0028/M cache
                  </div>
                )}
              </div>
              <div className="mt-2.5 flex items-center gap-2">
                <button
                  type="button"
                  disabled={syncingPrices}
                  onClick={async () => {
                    setSyncingPrices(true);
                    try {
                      const count = await api.aiSyncPrices();
                      const refreshed = await api.aiGetModelPrices();
                      setModelPrices(refreshed);
                      void notify("Pricing Synced", `Updated ${count} model rates from online catalog.`);
                    } catch (e) {
                      void notify("Sync Error", String(e));
                    } finally {
                      setSyncingPrices(false);
                    }
                  }}
                  className="flex-1 rounded border border-cyan-500/40 bg-cyan-950/40 px-2 py-1 text-[10px] text-cyan-300 hover:bg-cyan-900/50"
                >
                  {syncingPrices ? "Syncing live rates…" : "⚡ Sync Live Online Prices"}
                </button>
                <button
                  type="button"
                  onClick={() => setPicker(null)}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-3 py-1 text-[10px] text-[var(--text-dim)] hover:text-[var(--text)]"
                >
                  Close
                </button>
              </div>
            </div>
          )}
          {picker.kind === "vision-models" && (
            <CLIPicker
              title="Vision — model"
              options={visionModelOptions}
              onPick={onPickerPick}
              onCancel={() => {
                setPendingProviderId(null);
                setPicker(null);
              }}
              placeholder="Filter models…"
            />
          )}
          {picker.kind === "vision-ask" && (
            <CLIPicker
              title="Send images with this message?"
              options={visionAskOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
            />
          )}
          {picker.kind === "mode" && (
            <CLIPicker
              title="Agent Runtime Mode"
              options={modeOptions}
              onPick={onPickerPick}
              onCancel={() => setPicker(null)}
            />
          )}
        </div>
      )}

      <QueuedMessages
        items={queued}
        onChange={updateQueued}
        onRemove={removeQueued}
        canSendNow={!streaming}
        onSendNow={(id) => {
          const item = queued.find((q) => q.id === id);
          if (!item) return;
          removeQueued(id);
          enqueueOrSend(item.text, item.images);
        }}
      />

      {/* Composer — terminal prompt line */}
      <div className="border-t border-[var(--border)] px-3 pb-3 pt-2">
        {!activeProvider && (
          <div className="mb-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-300">
            No provider configured.{" "}
            <button className="underline" onClick={() => openSettings("providers")}>
              Add one
            </button>
            .
          </div>
        )}

        <div
          data-drop="agent-composer"
          className="relative rounded-md border border-[var(--border-strong)] bg-[var(--surface)] focus-within:border-[var(--accent)]"
          onDragOver={(e) => {
            if (e.dataTransfer.types.includes("Files")) e.preventDefault();
          }}
          onDrop={(e) => {
            const files = [...e.dataTransfer.files].filter((f) => f.type.startsWith("image/"));
            if (!files.length) return;
            e.preventDefault();
            void Promise.all(files.map((f) => fileToChatImage(f))).then((imgs) => addImages(imgs));
          }}
        >
          {/* Slash Commands Suggestion Menu */}
          {slashSuggestions.length > 0 && (
            <div className="absolute bottom-full left-0 right-0 z-30 mb-1.5 max-h-52 overflow-y-auto rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-2xl font-mono">
              <div className="flex items-center justify-between px-2 py-1 text-[10px] uppercase tracking-wider text-gray-500 font-sans">
                <span>Commands</span>
                <span>Tab / Enter to run</span>
              </div>
              {slashSuggestions.map((cmd, idx) => (
                <button
                  key={cmd.name}
                  type="button"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    pickSlashCommand(cmd);
                  }}
                  className={`flex w-full items-center justify-between rounded px-2 py-1 text-left text-[11px] transition ${
                    idx === slashIndex
                      ? "bg-[var(--border)] text-cyan-300 font-semibold"
                      : "text-gray-300 hover:bg-[var(--border)]/50"
                  }`}
                >
                  <span className="text-cyan-400">{cmd.syntax}</span>
                  <span className="truncate text-[10px] text-gray-400 font-sans">{cmd.description}</span>
                </button>
              ))}
            </div>
          )}

          {(pendingImages.length > 0 || pendingSnippets.length > 0) && (
            <div className="flex flex-wrap items-center gap-1.5 border-b border-[var(--border)]/60 bg-[var(--surface)]/40 px-2.5 py-1.5">
              {pendingImages.map((img, i) => (
                <div key={`${img.name}-${i}`} className="group relative">
                  <img
                    src={previewSrc(img)}
                    alt={img.name || `Image #${i + 1}`}
                    className="h-12 max-w-[88px] rounded border border-[var(--border)] object-cover"
                  />
                  <span className="absolute bottom-0 left-0 rounded-tr bg-black/60 px-1 text-[9px] text-white">
                    #{i + 1}
                  </span>
                  <button
                    type="button"
                    className="absolute -right-1 -top-1 hidden h-4 w-4 items-center justify-center rounded-full bg-[var(--surface)] text-[10px] text-red-300 group-hover:flex shadow"
                    onClick={() => setPendingImages((cur) => cur.filter((_, j) => j !== i))}
                    aria-label="Remove image"
                  >
                    ✕
                  </button>
                </div>
              ))}

              {pendingSnippets.map((snip, i) => (
                <div
                  key={snip.id}
                  className="group relative flex items-center gap-2 rounded-lg border border-cyan-500/30 bg-[#0a1120] px-2.5 py-1 text-xs shadow-sm transition hover:border-cyan-500/60"
                >
                  <span className="rounded bg-cyan-500/20 px-1.5 py-0.5 font-mono text-[10px] font-bold uppercase text-cyan-300">
                    {snip.language}
                  </span>
                  <button
                    type="button"
                    onClick={() => setPreviewSnippet(snip)}
                    className="flex flex-col text-left hover:text-cyan-200 transition"
                    data-tooltip="Click to view code snippet"
                  >
                    <span className="max-w-[130px] truncate font-mono text-[11px] font-medium text-gray-200">
                      {snip.name}
                    </span>
                    <span className="text-[9px] text-gray-400">
                      {snip.lineCount} lines · {(snip.size / 1024).toFixed(1)} KB
                    </span>
                  </button>
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      onClick={() => {
                        insertComposerText(snip.content);
                        setPendingSnippets((cur) => cur.filter((_, j) => j !== i));
                      }}
                      className="rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[9px] text-gray-400 hover:text-white"
                      data-tooltip="Expand to inline textarea"
                      aria-label="Insert snippet as text"
                    >
                      inline
                    </button>
                    <button
                      type="button"
                      onClick={() => setPendingSnippets((cur) => cur.filter((_, j) => j !== i))}
                      className="flex h-4 w-4 items-center justify-center rounded-full text-gray-400 hover:bg-red-500/20 hover:text-red-300"
                      aria-label="Remove snippet"
                      data-tooltip="Remove snippet"
                    >
                      ✕
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}

          <div className="flex items-start gap-2 px-2.5 py-2 font-mono">
            <span className="shrink-0 select-none font-mono text-[13px] leading-[21px] text-[var(--text-faint)]">
              ~#
            </span>
            <textarea
              ref={inputRef}
              value={input}
              rows={1}
              onChange={(e) => {
                setInput(e.target.value);
                history.record(e.target.value);
                recallIdx.current = null;
                setSlashIndex(0);
              }}
              onKeyDown={(e) => {
                const mod = e.ctrlKey || e.metaKey;
                // Ctrl+R — fast provider cycle
                if (mod && (e.key === "r" || e.key === "R")) {
                  e.preventDefault();
                  const enabled = providers.filter((p) => p.enabled);
                  if (enabled.length > 1) {
                    const idx = enabled.findIndex((p) => p.id === activeProvider?.id);
                    const next = enabled[(idx + 1) % enabled.length];
                    void useSettingsStore.getState().set("agent.active_provider", next.id);
                  } else if (enabled.length === 1) {
                    void useSettingsStore.getState().set("agent.active_provider", enabled[0].id);
                  }
                  return;
                }
                // Shift+Tab — toggle plan mode
                if (e.key === "Tab" && e.shiftKey) {
                  e.preventDefault();
                  togglePlanMode();
                  return;
                }
                // Open command palette with Ctrl/Cmd+K
                if (mod && (e.key === "k" || e.key === "K")) {
                  e.preventDefault();
                  setInput("/");
                  setSlashIndex(0);
                  return;
                }
                // Clear composer with Ctrl+L
                if (mod && (e.key === "l" || e.key === "L")) {
                  e.preventDefault();
                  setInput("");
                  history.reset("");
                  return;
                }
                // Undo / redo
                if (mod && (e.key === "z" || e.key === "Z")) {
                  e.preventDefault();
                  if (e.shiftKey) history.redo();
                  else history.undo();
                  return;
                }
                if (mod && (e.key === "y" || e.key === "Y")) {
                  e.preventDefault();
                  history.redo();
                  return;
                }
                // Slash commands keyboard navigation
                if (slashSuggestions.length > 0) {
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    setSlashIndex((i) => (i + 1) % slashSuggestions.length);
                    return;
                  }
                  if (e.key === "ArrowUp") {
                    e.preventDefault();
                    setSlashIndex((i) => (i - 1 + slashSuggestions.length) % slashSuggestions.length);
                    return;
                  }
                  if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
                    e.preventDefault();
                    const picked = slashSuggestions[slashIndex] || slashSuggestions[0];
                    if (picked) {
                      pickSlashCommand(picked);
                      return;
                    }
                  }
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setInput("");
                    return;
                  }
                }
                // Recall previously sent messages with Up/Down (shell-style)
                const userMsgs = messages
                  .filter((m) => m.role === "user")
                  .map((m) => m.content);
                if (
                  e.key === "ArrowUp" &&
                  !e.shiftKey &&
                  !mod &&
                  userMsgs.length > 0 &&
                  (input === "" || recallIdx.current !== null)
                ) {
                  e.preventDefault();
                  const cur = recallIdx.current ?? userMsgs.length;
                  const next = Math.max(0, cur - 1);
                  recallIdx.current = next;
                  setInput(userMsgs[next]);
                  history.record(userMsgs[next]);
                  return;
                }
                if (e.key === "ArrowDown" && !e.shiftKey && !mod && recallIdx.current !== null) {
                  e.preventDefault();
                  const next = recallIdx.current + 1;
                  if (next >= userMsgs.length) {
                    recallIdx.current = null;
                    setInput("");
                    history.record("");
                  } else {
                    recallIdx.current = next;
                    setInput(userMsgs[next]);
                    history.record(userMsgs[next]);
                  }
                  return;
                }
                if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                  e.preventDefault();
                  submit();
                }
              }}
              placeholder={
                streaming
                  ? "Queue a follow-up — you can edit it before it sends…"
                  : "Ask anything… (paste an image · / for commands · Enter to send)"
              }
              spellCheck={false}
              autoComplete="off"
              className="max-h-[132px] min-w-0 flex-1 resize-none border-0 bg-transparent p-0 text-[13px] leading-[21px] text-[var(--text)] outline-none placeholder:text-[var(--text-faint)] disabled:opacity-50"
            />
            <input
              ref={fileInputRef}
              type="file"
              accept="image/png,image/jpeg,image/gif,image/webp"
              multiple
              className="hidden"
              onChange={(e) => {
                const files = [...(e.target.files ?? [])];
                e.target.value = "";
                if (!files.length) return;
                void Promise.all(files.map((f) => fileToChatImage(f))).then((imgs) => addImages(imgs));
              }}
            />
            <button
              type="button"
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--text-faint)] hover:bg-[var(--border)] hover:text-[var(--text)]"
              data-tooltip="Attach image — Ctrl+V anywhere in this window, drop, or pick a file"
              onClick={() => fileInputRef.current?.click()}
              aria-label="Attach image"
            >
              <ImageIcon size={14} />
            </button>
          </div>

          {/* Voice status line (only when relevant). */}
          {(voiceStatus || voiceError || conversation) && (
            <div className="flex items-center gap-2 border-t border-[var(--border)]/60 px-2 pb-1 pt-1">
              {voiceStatus && (
                <span className="truncate text-[10px] text-[var(--text-dim)]" data-tooltip={voiceStatus}>
                  {voiceStatus}
                </span>
              )}
              {voiceError && !voiceStatus && (
                <span className="truncate text-[10px] text-red-400" data-tooltip={voiceError}>
                  {voiceError}
                </span>
              )}
              {conversation && (
                <span className="truncate text-[10px] text-emerald-400" data-tooltip="Conversation mode active">
                  listening…
                </span>
              )}
            </div>
          )}

          {/* Composer input bar: targets · provider·model · reasoning · plan · permissions · ctx · cost · git · send/stop */}
          <InputBar
            activeTargets={activeTargetsInfo}
            onPickTargets={handlePickTargets}
            activeProvider={activeProvider}
            activeModel={activeModel || undefined}
            reasoning={reasoning}
            onReasoning={setReasoningPersisted}
            onPickReasoning={handlePickReasoning}
            planMode={planMode}
            onTogglePlan={togglePlanMode}
            agentMode={agentMode}
            onPickMode={() => setPicker({ kind: "mode" })}
            safetyMode={effectiveSafetyMode}
            onCycleSafety={handleCycleSafety}
            onPickSafety={handlePickSafety}
            contextUsage={contextUsage}
            streamStats={displayStats}
            sessionCache={sessionCache}
            costUsd={conversationCostUsd}
            gitLabel={gitLabel}
            streaming={streaming}
            onSend={submit}
            onStop={handleStop}
            onPickModel={handlePickModel}
            onPickContext={handlePickContext}
            visionLabel={visionLabel(
              visionMode,
              visionProvider?.name,
              visionModelLabel || undefined,
            )}
            onPickVision={handlePickVision}
          />

        </div>

      </div>
      </div>

      {previewSnippet && (
        <SnippetPreviewModal
          snippet={previewSnippet}
          onClose={() => setPreviewSnippet(null)}
          onInsertInline={(code) => insertComposerText(code)}
          onDelete={() => {
            setPendingSnippets((cur) => cur.filter((s) => s.id !== previewSnippet.id));
            setPreviewSnippet(null);
          }}
        />
      )}

      {showTrajectory && (
        <TrajectoryModal onClose={() => setShowTrajectory(false)} />
      )}

    </div>

  );
});


