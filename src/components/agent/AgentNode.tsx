import { useEffect, useMemo, useRef, useState } from "react";
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

import { api } from "../../lib/tauri";

import { useUiStore } from "../../stores/uiStore";

import { useVpsStore } from "../../stores/vpsStore";
import { useCanvasStore, NODE_W, NODE_H, type AgentNode as AgentNodeType } from "../../stores/canvasStore";

import { useSettingsStore } from "../../stores/settingsStore";

import { AgentMarkdown } from "./AgentMarkdown";
import { AgentConsole } from "./AgentConsole";
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
import { PROVIDER_CATALOG } from "../../lib/providerCatalog";
import { InputBar, type ReasoningLevel } from "./InputBar";
import { useGitBranch } from "../../hooks/useGitBranch";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { effectiveMode, shouldAutoRun } from "../../lib/safety";

import type { AgentApproval, AgentPlan, AgentQuestion } from "../../lib/tauri";

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



// ---- Interactive popups (Claude-Code-style) -------------------------------

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

function PlanCard({
  plan,
  onResolve,
}: {
  plan: AgentPlan;
  onResolve: (id: string, approve: boolean, feedback?: string) => void;
}) {
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedback, setFeedback] = useState("");

  return (
    <div className="mb-2 rounded-md border border-emerald-500/40 bg-emerald-500/10 p-2 last:mb-0">
      <div className="mb-1 text-[11px] font-medium text-emerald-200">
        {plan.title ? `Plan: ${plan.title}` : "Review this plan"}
      </div>
      <div className="mb-2 max-h-64 overflow-auto rounded bg-[var(--bg)] px-2 py-1.5 text-[12px] text-gray-200">
        <AgentMarkdown content={plan.plan} variant="assistant" />
      </div>
      {showFeedback ? (
        <div className="flex flex-col gap-1.5">
          <textarea
            value={feedback}
            onChange={(e) => setFeedback(e.target.value)}
            rows={3}
            placeholder="What should change?"
            className="w-full resize-none rounded border border-[var(--border-strong)] bg-[var(--bg)] px-2 py-1 text-[11px] text-gray-200 outline-none placeholder:text-gray-600 focus:border-[#3d4a61]"
          />
          <div className="flex justify-end gap-2">
            <button
              onClick={() => setShowFeedback(false)}
              className="rounded-md border border-[var(--border)] px-2.5 py-1 text-[11px] text-gray-300 hover:bg-[var(--border)]"
            >
              Cancel
            </button>
            <button
              onClick={() => onResolve(plan.id, false, feedback)}
              className="rounded-md bg-amber-600 px-2.5 py-1 text-[11px] text-white hover:bg-amber-500"
            >
              Send changes
            </button>
          </div>
        </div>
      ) : (
        <div className="flex justify-end gap-2">
          <button
            onClick={() => setShowFeedback(true)}
            className="rounded-md border border-[var(--border)] px-2.5 py-1 text-[11px] text-gray-300 hover:bg-[var(--border)]"
          >
            Request changes
          </button>
          <button
            onClick={() => onResolve(plan.id, true)}
            className="rounded-md bg-emerald-600 px-2.5 py-1 text-[11px] text-white hover:bg-emerald-500"
          >
            Approve &amp; run
          </button>
        </div>
      )}
    </div>
  );
}



export function AgentNodeView({ id, selected }: NodeProps<AgentNodeType>) {
  const openSettings = useUiStore((s) => s.openSettings);

  // Node chrome: focus on click, drag by header (React Flow), tile counter-scale.
  const focus = useCanvasStore((s) => s.focus);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);



  const {

    sessionId,

    messages,

    conversations,

    streamingText,

    activity,

    streamStats,

    turnTelemetry,

    prefixTelemetry,

    contextUsage,

    conversationCostUsd,

    streaming,

    error,

    targets,

    pendingApprovals,

    pendingQuestions,

    pendingPlan,

    planMode,

    send,

    retryLast, clearError, setTargets,

    togglePlanMode,
    stop,

    init,

    newConversation,

    openConversation,

    exportConversationMarkdown,

    resolveApproval,

    answerQuestion,

    resolvePlan,

  } = useAgentStore();



  const vpsList = useVpsStore((s) => s.vpsList);

  const loadVps = useVpsStore((s) => s.load);

  const loadSettings = useSettingsStore((s) => s.load);

  const providers = useSettingsStore((s) => s.providers);

  const activeProviderId = useSettingsStore((s) => s.settings["agent.active_provider"]);
  const activeModel = useSettingsStore((s) => s.settings["agent.active_model"]);

  // Reasoning effort (t3code-style capability control), persisted.
  const [reasoning, setReasoning] = useState<ReasoningLevel>(() => {
    const v = useSettingsStore.getState().settings["agent.reasoning_level"];
    return v === "low" || v === "medium" || v === "high" || v === "off" ? v : "off";
  });
  const setReasoningPersisted = (r: ReasoningLevel) => {
    setReasoning(r);
    void useSettingsStore.getState().set("agent.reasoning_level", r);
  };

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
  const executeCommand = (code: string) => {
    const canvas = useCanvasStore.getState();
    const vpsList = useVpsStore.getState().vpsList;
    // Resolve target: the agent's selected targets first, else the first vps.
    const targetId = targets[0] ?? vpsList[0]?.id;
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
  };

  /** VPS context passed to code blocks so Execute can show the target name. */
  const executeTarget = useMemo(() => {
    const vpsList = useVpsStore.getState().vpsList;
    const targetId = targets[0] ?? vpsList[0]?.id;
    const vps = vpsList.find((v) => v.id === targetId);
    return vps ? { name: vps.name, host: vps.host } : null;
  }, [targets]);



  const [input, setInput] = useState("");

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
    try {
      const key = `xconsole-agent-draft:${sessionId}`;
      if (input) localStorage.setItem(key, input);
      else localStorage.removeItem(key);
    } catch {
      /* ignore */
    }
  }, [input, sessionId]);

  const history = useInputHistory(setInput);

  // Up/Down recalls previously sent user messages (shell-style). null = not recalling.
  const recallIdx = useRef<number | null>(null);
  // Mirrors the picker state so the Escape handler (declared before the state)
  // can see whether a picker is open.
  const pickerOpenRef = useRef(false);

  // Escape closes whatever is open first (picker → stop → window), never jumps
  // straight to closing the whole agent window.
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
      // 3. Nothing else open → close the agent window.
      useCanvasStore.getState().removeNode(id);
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

  // In-console picker (CLI style): /model (two-level), /targets, /history, /ctx, /cost, /help.
  type PickerKind =
    | "model"
    | "model-models"
    | "targets"
    | "history"
    | "ctx"
    | "cost"
    | "help";
  const [picker, setPicker] = useState<{ kind: PickerKind } | null>(null);
  /** Provider id chosen in the first /model level — second level lists its models. */
  const [pendingProviderId, setPendingProviderId] = useState<string | null>(null);
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

  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

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

    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });

  }, [messages, streamingText, activity]);



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
        detail: v.host,
        selected: targets.includes(v.id),
      })),
    [vpsList, targets],
  );

  const historyOptions = useMemo<CLIPickerOption[]>(
    () =>
      conversations.map((c) => ({
        id: c.id,
        label: c.title || c.id.slice(0, 8),
        detail: c.id === sessionId ? "current" : undefined,
      })),
    [conversations, sessionId],
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

  /** Options for the /model picker's SECOND level: models of the chosen provider. */
  const providerModelOptions = useMemo<CLIPickerOption[]>(() => {
    const p = providers.find((x) => x.id === pendingProviderId);
    if (!p) return [];
    const catalog = PROVIDER_CATALOG.find((c) => c.id === p.kind || c.name.toLowerCase() === p.name.toLowerCase());
    const ids = new Set<string>();
    const opts: CLIPickerOption[] = [];
    if (p.model) {
      ids.add(p.model);
      opts.push({ id: p.model, label: p.model, detail: "configured", selected: p.model === activeModel });
    }
    for (const m of catalog?.models ?? []) {
      if (ids.has(m)) continue;
      ids.add(m);
      opts.push({ id: m, label: m, detail: "catalog" });
    }
    return opts;
  }, [providers, pendingProviderId, activeModel]);

  /** Handle a picker selection. */
  const onPickerPick = (opt: CLIPickerOption) => {
    if (!picker) return;
    switch (picker.kind) {
      case "model": {
        // First level: provider chosen → second level lists its models.
        setPendingProviderId(opt.id);
        setPicker({ kind: "model-models" });
        break;
      }
      case "model-models": {
        // Second level: model chosen → set provider + model.
        void useSettingsStore.getState().set("agent.active_provider", pendingProviderId ?? "");
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
    } else if (cmd.actionKey === "model") {
      setPicker({ kind: "model" });
    } else if (cmd.actionKey === "targets") {
      setPicker({ kind: "targets" });
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
    }
  };

  const submit = () => {
    const trimmed = input.trim();
    if (!trimmed) return;
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
    send(input);
    setInput("");
    history.reset("");
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



  return (
    <div
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-blue-500" : "border-[var(--border)]"}`}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
      onMouseDown={() => focus(id)}
    >
      <NodeResizer
        minWidth={340}
        minHeight={240}
        isVisible
        lineClassName="!border-blue-500"
        handleClassName="!bg-blue-500"
      />

      {/* Slim terminal status line (no buttons — everything is a command). */}
      <div className="flex cursor-move select-none items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 font-mono text-[11px]">
        <span className="text-cyan-400">{streaming ? "●" : "❯"}</span>
        <span className="text-[var(--text-dim)]">agent</span>
        <span className="text-[var(--text-faint)]">·</span>
        <button
          type="button"
          onClick={() => setPicker({ kind: "model" })}
          onMouseDown={(e) => e.stopPropagation()}
          data-tooltip="Switch provider (/model)"
          className="truncate text-gray-300 hover:text-cyan-300"
        >
          {activeProvider?.name ?? "no provider"}
          {activeModel || activeProvider?.model ? ` · ${activeModel || activeProvider?.model}` : ""}
        </button>
        {planMode && <span className="rounded bg-indigo-500/20 px-1 text-[9px] text-indigo-300">plan</span>}
        {loopTask && (
          <span className="rounded bg-cyan-500/20 px-1 text-[9px] text-cyan-300">
            ⟳ loop {loopCount}/{loopMax}
          </span>
        )}
        {ttsEnabled && <span className="text-[9px] text-[var(--text-faint)]">🔊</span>}
        <span className="ml-auto flex items-center gap-1">
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
        </span>
        <span className="flex items-center gap-2 text-[var(--text-faint)]">
          {contextUsage ? <span>{contextUsage.percent}% ctx</span> : null}
          {conversationCostUsd > 0 ? (
            <span>${conversationCostUsd.toFixed(4)}</span>
          ) : null}
          {streaming ? (
            <span className="animate-pulse text-emerald-400">working…</span>
          ) : null}
        </span>
      </div>

      {/* Body: nodrag so only the header starts a node drag (like TerminalNode) —
          text selection inside the console/composer works normally. */}
      <div className="nodrag flex min-h-0 flex-1 flex-col">
      {/* Messages */}
      {messages.length === 0 && !streaming ? (
        <div className="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-xs text-gray-600">
          <div className="space-y-2 font-mono">
            <p className="text-[var(--text-dim)]">agent@xconsole:~$</p>
            <p className="text-[10px] text-gray-700">
              Type a task, or /help for commands. /model picks the provider · /targets
              selects hosts · Shift+Tab toggles plan mode.
            </p>
          </div>
        </div>
      ) : (
        <AgentConsole
          messages={messages}
          streamingText={streamingText}
          streaming={streaming}
          streamStats={streamStats}
          turnTelemetry={turnTelemetry}
          prefixTelemetry={prefixTelemetry}
          expanded
          executeTarget={executeTarget}
          onExecute={executeCommand}
          fontSize={consoleFontSize}
        />
      )}

      {error && (
        <div className="flex items-start gap-2 border-t border-red-900/30 px-3 py-2 text-xs text-red-400">
          <span className="min-w-0 flex-1">{error}</span>
          <button
            type="button"
            className="shrink-0 rounded border border-red-800/50 px-1.5 py-0.5 text-[10px] text-red-200 hover:bg-red-950/50"
            onClick={() => void retryLast()}
            data-tooltip="Re-send the last message"
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

      {/* Interactive prompts: approvals, questions, plan review */}
      {(pendingApprovals.length > 0 ||
        pendingQuestions.length > 0 ||
        pendingPlan) && (
        <div className="border-t border-[var(--border)] bg-[var(--bg)] px-3 py-2">
          {pendingApprovals.map((a) => (
            <ApprovalCard key={a.id} approval={a} onResolve={resolveApproval} />
          ))}
          {pendingQuestions.map((q) => (
            <QuestionCard key={q.id} question={q} onAnswer={answerQuestion} />
          ))}
          {pendingPlan && (
            <PlanCard plan={pendingPlan} onResolve={resolvePlan} />
          )}
        </div>
      )}

      {/* In-console picker (CLI style) */}
      {picker && (
        <div className="border-t border-[var(--border)] px-3 pb-2 pt-2">
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
              <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
                Context
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
              <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
                Cost
              </div>
              <div className="text-[var(--text)]">
                This conversation: ${conversationCostUsd.toFixed(4)}
              </div>
              <div className="text-[var(--text-faint)]">
                {streamStats?.promptTokens
                  ? `last turn: ${streamStats.promptTokens.toLocaleString()} in · ${streamStats.completionTokens.toLocaleString()} out`
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
        </div>
      )}

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

        <div className="relative rounded-md border border-[var(--border-strong)] bg-[var(--surface)] focus-within:border-[var(--accent)]">
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
                    void executeSlashAction(cmd);
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

          <div className="flex items-start gap-2 px-2.5 py-2 font-mono">
            <span className="shrink-0 pt-1.5 text-[13px] text-[var(--text-faint)]">›</span>
            <textarea
              ref={inputRef}
              value={input}
              rows={1}
              onChange={(e) => {
                setInput(e.target.value);
                history.record(e.target.value);
                recallIdx.current = null;
                setSlashIndex(0);
                // Auto-grow up to 6 lines.
                e.target.style.height = "auto";
                e.target.style.height = `${Math.min(e.target.scrollHeight, 132)}px`;
              }}
              onKeyDown={(e) => {
                const mod = e.ctrlKey || e.metaKey;
                // Ctrl+R — fast provider cycle (Claude Code-style)
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
                // Shift+Tab — toggle plan mode (Claude Code-style)
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
                      void executeSlashAction(picked);
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
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  submit();
                }
              }}
              placeholder="Ask anything… (/ for commands · Enter to send · Shift+Enter for new line)"
              disabled={streaming}
              spellCheck={false}
              autoComplete="off"
              className="max-h-[132px] min-w-0 flex-1 resize-none border-0 bg-transparent text-[13px] leading-relaxed text-[var(--text)] outline-none placeholder:text-[var(--text-faint)] disabled:opacity-50"
            />
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

          {/* t3code-style input bar: provider·model · reasoning · plan · permissions · ctx · cost · git · send/stop */}
          <InputBar
            activeProvider={activeProvider}
            activeModel={activeModel || undefined}
            reasoning={reasoning}
            onReasoning={setReasoningPersisted}
            planMode={planMode}
            onTogglePlan={togglePlanMode}
            safetyMode={useSettingsStore((s) => s.settings["agent.safety_mode"]) ?? "approve"}
            onCycleSafety={() => {
              const settings = useSettingsStore.getState().settings;
              const cur = settings["agent.safety_mode"] ?? "approve";
              const next = cur === "full" ? "allowlist" : cur === "allowlist" ? "approve" : "full";
              void useSettingsStore.getState().set("agent.safety_mode", next);
            }}
            contextUsage={contextUsage}
            streamStats={streamStats}
            costUsd={conversationCostUsd}
            gitLabel={gitLabel}
            streaming={streaming}
            onSend={submit}
            onStop={() => {
              setLoopTask(null);
              void stop();
            }}
            onPickModel={() => setPicker({ kind: "model" })}
          />

        </div>

      </div>
      </div>

    </div>

  );

}


