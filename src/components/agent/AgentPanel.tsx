import { useEffect, useMemo, useRef, useState } from "react";

import { useAgentStore } from "../../stores/agentStore";

import { useInputHistory } from "../../hooks/useInputHistory";

import { useVoiceStore } from "../../stores/voiceStore";

import {
  startRecording,
  startConversation,
  cancelSpeech,
  isSpeaking,
  type Recorder,
  type Conversation,
} from "../../lib/voice";

import { api } from "../../lib/tauri";

import { useUiStore } from "../../stores/uiStore";

import { useVpsStore } from "../../stores/vpsStore";

import { useSettingsStore } from "../../stores/settingsStore";

import {
  BotIcon,
  ChevronDownIcon,
  ConversationIcon,
  LoaderIcon,
  MaximizeIcon,
  MicIcon,
  MinimizeIcon,
  PlanIcon,
  SettingsIcon,
  StopIcon,
  TrashIcon,
  VolumeIcon,
  VolumeOffIcon,
} from "../icons";

import { AgentMarkdown } from "./AgentMarkdown";

import { AgentActivityFeed, AgentThinking } from "./AgentActivity";
import { AgentTokenStats } from "./AgentTokenStats";
import { AgentContextUsageButton } from "./AgentContextUsage";

import { AgentHistory } from "./AgentHistory";

import type { AgentChatMessage } from "../../stores/agentStore";

import type { AgentApproval, AgentPlan, AgentQuestion } from "../../lib/tauri";



function MessageBubble({

  role,

  content,

  activity,

  tokenStats,

  wide,

}: {

  role: string;

  content: string;

  activity?: AgentChatMessage["activity"];

  tokenStats?: AgentChatMessage["tokenStats"];

  wide?: boolean;

}) {

  const isUser = role === "user";

  return (

    <div className={`flex flex-col gap-2 ${isUser ? "items-end" : "items-start"}`}>

      {!isUser && activity && activity.length > 0 && (
        <div className={wide ? "w-full max-w-full" : "w-[88%] max-w-[88%]"}>
          <AgentActivityFeed items={activity} />
        </div>
      )}

      <div className={`flex ${isUser ? "justify-end" : "justify-start"} w-full`}>

        <div

          className={`rounded-lg px-3 py-2 text-sm leading-relaxed ${

            wide ? "max-w-full" : "max-w-[88%]"

          } ${

            isUser

              ? "bg-blue-600 text-white"

              : "border border-[var(--border)] bg-[var(--surface)] text-gray-200"

          }`}

        >

          <AgentMarkdown content={content} variant={isUser ? "user" : "assistant"} />

        </div>

      </div>

      {!isUser && tokenStats && (
        <div className={wide ? "w-full pl-1" : "w-[88%] pl-1"}>
          <AgentTokenStats stats={tokenStats} />
        </div>
      )}

    </div>

  );

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

const CLI_KINDS = new Set(["cursor", "codex_cli", "opencode_cli"]);

const TOOL_KINDS = new Set(["openai", "anthropic", "ollama"]);

const CURSOR_KIND = "cursor";



/** Compact in-chat switcher for the active agent provider/model. Updates the
 *  `agent.active_provider` setting, which the agent reads on the next turn. */
function ProviderSwitcher() {
  const providers = useSettingsStore((s) => s.providers);
  const activeId = useSettingsStore((s) => s.settings["agent.active_provider"]);
  const setSetting = useSettingsStore((s) => s.set);
  const openSettings = useUiStore((s) => s.openSettings);
  const [open, setOpen] = useState(false);

  const enabled = providers.filter((p) => p.enabled);
  const active = providers.find((p) => p.id === activeId) ?? enabled[0];

  if (enabled.length === 0) {
    return (
      <button
        onClick={() => openSettings("providers")}
        className="rounded-md px-1.5 py-0.5 text-[11px] text-amber-300 hover:bg-[var(--border)]"
      >
        + Add provider
      </button>
    );
  }

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        onBlur={() => setTimeout(() => setOpen(false), 150)}
        data-tooltip="Switch the agent's provider / model"
        className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
      >
        <span className="max-w-[150px] truncate">{active?.name ?? "no provider"}</span>
        <ChevronDownIcon size={11} />
      </button>
      {open && (
        <div className="absolute left-0 top-full z-30 mt-1 max-h-72 w-60 overflow-auto rounded-md border border-[var(--border)] bg-[var(--surface)] py-1 shadow-xl">
          {enabled.map((p) => (
            <button
              key={p.id}
              onMouseDown={(e) => {
                e.preventDefault();
                void setSetting("agent.active_provider", p.id);
                setOpen(false);
              }}
              className={`flex w-full flex-col items-start px-2.5 py-1.5 text-left hover:bg-[var(--border)] ${
                p.id === active?.id ? "bg-[var(--border)]/60" : ""
              }`}
            >
              <span className="text-[11px] text-[var(--text)]">{p.name}</span>
              <span className="truncate text-[10px] text-[var(--text-faint)]">
                {p.model || p.kind}
              </span>
            </button>
          ))}
          <div className="my-1 border-t border-[var(--border)]" />
          <button
            onMouseDown={(e) => {
              e.preventDefault();
              openSettings("providers");
              setOpen(false);
            }}
            className="w-full px-2.5 py-1.5 text-left text-[10px] text-[var(--text-dim)] hover:bg-[var(--border)]"
          >
            Manage providers…
          </button>
        </div>
      )}
    </div>
  );
}

export function AgentPanel({ expanded = false }: { expanded?: boolean }) {

  const open = useUiStore((s) => s.agentOpen);

  const setAgentOpen = useUiStore((s) => s.setAgentOpen);

  const toggleAgentExpanded = useUiStore((s) => s.toggleAgentExpanded);

  const setAgentExpanded = useUiStore((s) => s.setAgentExpanded);

  const openSettings = useUiStore((s) => s.openSettings);



  const {

    sessionId,

    messages,

    conversations,

    streamingText,

    activity,

    streamStats,

    contextUsage,

    compactFlipCount,

    streaming,

    speaking,

    error,

    targets,

    pendingApprovals,

    pendingQuestions,

    pendingPlan,

    planMode,

    send,

    setTargets,

    togglePlanMode,
    stop,

    init,

    newConversation,

    openConversation,

    removeConversation,

    resolveApproval,

    answerQuestion,

    resolvePlan,

  } = useAgentStore();



  const vpsList = useVpsStore((s) => s.vpsList);

  const loadVps = useVpsStore((s) => s.load);

  const loadSettings = useSettingsStore((s) => s.load);

  const providers = useSettingsStore((s) => s.providers);

  const activeProviderId = useSettingsStore((s) => s.settings["agent.active_provider"]);



  const [input, setInput] = useState("");

  const history = useInputHistory(setInput);

  // Up/Down recalls previously sent user messages (shell-style). null = not recalling.
  const recallIdx = useRef<number | null>(null);

  // Voice: mic capture + spoken replies.
  const recording = useVoiceStore((s) => s.recording);
  const transcribing = useVoiceStore((s) => s.transcribing);
  const ttsEnabled = useVoiceStore((s) => s.ttsEnabled);
  const recorderRef = useRef<Recorder | null>(null);
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

  const toggleMic = async () => {
    const vs = useVoiceStore.getState();
    if (vs.recording) {
      const rec = recorderRef.current;
      recorderRef.current = null;
      vs.setRecording(false);
      if (!rec) return;
      vs.setTranscribing(true);
      try {
        const wav = await rec.stop();
        const text = await transcribeAuto(wav);
        if (text.trim()) {
          const next = input.trim() ? `${input} ${text}` : text;
          if (vs.autoSend) {
            // Spoken turns use the dedicated conversation model when one is set.
            send(next, { providerId: vs.conversationProvider || undefined });
            setInput("");
            history.reset("");
          } else {
            setInput(next);
            history.record(next);
          }
        }
        setVoiceError("");
      } catch (e) {
        setVoiceError(String(e));
      } finally {
        setVoiceStatus("");
        vs.setTranscribing(false);
      }
    } else {
      try {
        recorderRef.current = await startRecording();
        useVoiceStore.getState().setRecording(true);
        setVoiceError("");
      } catch {
        setVoiceError("Microphone access was blocked. Allow the mic for this app and try again.");
      }
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

  const [showTargets, setShowTargets] = useState(false);

  const [showHistory, setShowHistory] = useState(false);

  const [showContextUsage, setShowContextUsage] = useState(false);

  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);



  useEffect(() => {

    if (open) {

      loadVps();

      loadSettings();

      void init();

    }

  }, [open, loadVps, loadSettings, init]);



  useEffect(() => {

    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });

  }, [messages, streamingText, activity]);



  useEffect(() => {

    const el = inputRef.current;

    if (!el) return;

    el.style.height = "auto";

    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;

  }, [input]);



  const activeProvider = useMemo(

    () => providers.find((p) => p.id === activeProviderId) ?? providers[0],

    [providers, activeProviderId],

  );



  const hasToolProvider = useMemo(

    () => providers.some((p) => p.enabled && TOOL_KINDS.has(p.kind)),

    [providers],

  );



  const cliNeedsApi = useMemo(() => {

    if (!activeProvider || !CLI_KINDS.has(activeProvider.kind)) return false;

    if (activeProvider.kind === CURSOR_KIND) return false;

    return !hasToolProvider;

  }, [activeProvider, hasToolProvider]);



  const submit = () => {

    if (!input.trim()) return;

    send(input);

    setInput("");

    history.reset("");

    recallIdx.current = null;

  };



  const toggleTarget = (id: string) =>

    setTargets(

      targets.includes(id) ? targets.filter((t) => t !== id) : [...targets, id],

    );



  if (!open && !expanded) return null;



  return (

    <aside

      className={`flex shrink-0 flex-col border-[var(--border)] bg-[var(--surface-2)] ${

        expanded

          ? "h-full min-w-0 flex-1 border-0"

          : "h-full w-[420px] border-l"

      }`}

    >

      <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-2.5">

        <BotIcon size={16} />

        <span className="text-xs font-medium uppercase tracking-wider text-gray-300">

          Agent

        </span>

        <ProviderSwitcher />

        <div className="ml-auto flex items-center gap-1">

          <button

            className="rounded-md px-1.5 py-1 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 disabled:opacity-40 disabled:pointer-events-none"

            data-tooltip={streaming ? "Finish the current turn first" : "Chat history"}

            disabled={streaming}

            onClick={() => setShowHistory((v) => !v)}

          >

            History

          </button>

          <button

            className="rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 disabled:opacity-40 disabled:pointer-events-none"

            data-tooltip={streaming ? "Finish the current turn first" : "New conversation"}

            disabled={streaming}

            onClick={() => void newConversation()}

          >

            <TrashIcon size={15} />

          </button>

          <button

            className="rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"

            data-tooltip="Agent settings"

            onClick={() => openSettings("agent")}

          >

            <SettingsIcon size={15} />

          </button>

          <button

            className="rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"

            data-tooltip={expanded ? "Exit fullscreen" : "Expand to full window"}

            onClick={() => expanded ? setAgentExpanded(false) : toggleAgentExpanded()}

          >

            {expanded ? <MinimizeIcon size={15} /> : <MaximizeIcon size={15} />}

          </button>

          {!expanded && (

            <button

              className="rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"

              data-tooltip="Close"

              onClick={() => setAgentOpen(false)}

            >

              ✕

            </button>

          )}

        </div>

      </div>



      <AgentHistory

        open={showHistory}

        conversations={conversations}

        activeId={sessionId}

        onSelect={(id) => {

          void openConversation(id);

          setShowHistory(false);

        }}

        onNew={() => {

          void newConversation();

          setShowHistory(false);

        }}

        onDelete={(id) => void removeConversation(id)}

        onClose={() => setShowHistory(false)}

      />



      {/* Targets */}

      <div className="border-b border-[var(--border)] px-3 py-1.5">

        <button

          className="flex w-full items-center gap-2 text-[11px] text-gray-400 hover:text-gray-200"

          onClick={() => setShowTargets((v) => !v)}

        >

          <span>

            VPS targets:{" "}

            <span className="text-gray-200">

              {targets.length === 0 ? "none (infra-only mode)" : `${targets.length} selected`}

            </span>

          </span>

          <span className="ml-auto">{showTargets ? "▲" : "▼"}</span>

        </button>

        {showTargets && (

          <div className="mt-1.5 flex flex-wrap gap-1 pb-1">

            <button

              onClick={() => setTargets(vpsList.map((v) => v.id))}

              className="rounded-full border border-[var(--border)] px-2 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"

            >

              All

            </button>

            <button

              onClick={() => setTargets([])}

              className="rounded-full border border-[var(--border)] px-2 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"

            >

              None

            </button>

            {vpsList.map((v) => (

              <button

                key={v.id}

                onClick={() => toggleTarget(v.id)}

                className={`rounded-full border px-2 py-0.5 text-[10px] ${

                  targets.includes(v.id)

                    ? "border-blue-500 bg-blue-600/30 text-blue-100"

                    : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)]"

                }`}

              >

                {v.name}

              </button>

            ))}

            {targets.length === 0 && (

              <p className="w-full text-[10px] leading-relaxed text-gray-500">

                No VPS selected — agent can still manage Terraform projects locally or via

                Terraform Cloud. Select VPS for SSH commands and remote terraform runner.

              </p>

            )}

          </div>

        )}

      </div>



      {/* Messages */}

      <div ref={scrollRef} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-3">

        {messages.length === 0 && !streaming && (

          <div className="mt-6 space-y-2 text-center text-xs text-gray-600">

            <p>Ask the agent to inspect, fix, or automate your servers.</p>

            <p className="text-[10px] text-gray-700">

              Select VPS targets, then ask — commands run live and show in the activity feed.

            </p>

          </div>

        )}

        {messages.map((m, i) => (

          <MessageBubble

            key={i}

            role={m.role}

            content={m.content}

            activity={m.activity}

            tokenStats={m.tokenStats}

            wide={expanded}

          />

        ))}



        {streaming && (

          <div className={`flex flex-col gap-2 ${expanded ? "w-full" : "w-[88%]"}`}>

            {!streamingText && activity.length === 0 && <AgentThinking />}

            {activity.length > 0 && (
              <div className="w-full">
                <AgentActivityFeed items={activity} live />
              </div>
            )}

            {streamingText && (
              <>
                <MessageBubble role="assistant" content={streamingText} wide={expanded} />
                {streamStats && (
                  <div className={`pl-1 ${expanded ? "w-full" : "w-[88%]"}`}>
                    <AgentTokenStats stats={streamStats} live />
                  </div>
                )}
              </>
            )}

          </div>

        )}

        {error && <div className="text-xs text-red-400">{error}</div>}

      </div>



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



      {/* Composer */}

      <div className="border-t border-[var(--border)] p-3">

        {!activeProvider && (

          <div className="mb-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-300">

            No provider configured.{" "}

            <button className="underline" onClick={() => openSettings("providers")}>

              Add one

            </button>

            .

          </div>

        )}

        {cliNeedsApi && targets.length > 0 && (

          <div className="mb-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-300">

            {activeProvider?.name} is chat-only and cannot SSH.{" "}

            <button className="underline" onClick={() => openSettings("providers")}>

              Add OpenAI or Anthropic

            </button>{" "}

            to run commands on your VPS (Full autonomy applies automatically).

          </div>

        )}

        {activeProvider &&

          CLI_KINDS.has(activeProvider.kind) &&

          activeProvider.kind !== CURSOR_KIND &&

          hasToolProvider &&

          targets.length > 0 && (

            <div className="mb-2 rounded-md border border-blue-500/20 bg-blue-500/5 px-2 py-1 text-[10px] text-blue-200/80">

              CLI provider active — xConsole will use your API provider to execute SSH/tools.

            </div>

          )}

        <div className="relative rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] shadow-[inset_0_1px_0_rgba(255,255,255,0.03)] focus-within:border-[var(--accent)]">

          <textarea

            ref={inputRef}

            value={input}

            onChange={(e) => {
              setInput(e.target.value);
              history.record(e.target.value);
              recallIdx.current = null;
            }}

            onKeyDown={(e) => {
              const mod = e.ctrlKey || e.metaKey;
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

            rows={1}

            placeholder="Ask anything…  (Enter to send · Shift+Enter for a new line)"

            disabled={streaming}

            className="block w-full resize-none border-0 bg-transparent pl-3.5 pr-10 pb-1.5 pt-3 text-[13px] leading-relaxed text-[var(--text)] outline-none placeholder:text-[var(--text-faint)] disabled:opacity-50"

            style={{ minHeight: "44px", maxHeight: "160px" }}

          />

          {/* Footer: plan-mode + voice controls on the left (Enter sends) */}
          <div className="flex items-center gap-1.5 px-2 pb-2 pt-0.5">

            <button
              onClick={togglePlanMode}
              data-tooltip="Plan mode: the agent investigates and proposes a plan for your approval before changing anything."
              aria-label="Plan mode"
              className={`flex items-center justify-center rounded-md p-1.5 transition ${
                planMode
                  ? "bg-indigo-600/30 text-indigo-200 ring-1 ring-indigo-500/50"
                  : "text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
              }`}
            >
              <PlanIcon size={16} />
            </button>

            <button
              onClick={toggleMic}
              disabled={transcribing}
              data-tooltip={recording ? "Stop & transcribe" : "Speak (voice input)"}
              aria-label={recording ? "Stop recording" : "Speak"}
              className={`flex items-center justify-center rounded-md p-1.5 transition disabled:opacity-60 ${
                recording
                  ? "bg-red-600/30 text-red-300 ring-1 ring-red-500/50"
                  : "text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
              }`}
            >
              {transcribing ? (
                <LoaderIcon size={16} className="animate-spin" />
              ) : recording ? (
                <StopIcon size={14} className="animate-pulse" />
              ) : (
                <MicIcon size={16} />
              )}
            </button>

            <button
              onClick={toggleSpeaker}
              data-tooltip={ttsEnabled ? "Spoken replies on — click to mute" : "Speak replies aloud"}
              aria-label={ttsEnabled ? "Mute spoken replies" : "Enable spoken replies"}
              className={`flex items-center justify-center rounded-md p-1.5 transition ${
                ttsEnabled
                  ? "bg-[var(--border)] text-[var(--text)]"
                  : "text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
              }`}
            >
              {ttsEnabled ? <VolumeIcon size={16} /> : <VolumeOffIcon size={16} />}
            </button>

            <button
              onClick={toggleConversation}
              data-tooltip={
                conversation
                  ? "Conversation mode on — listening continuously. Click to stop."
                  : "Conversation mode: talk hands-free, it replies aloud and keeps listening."
              }
              aria-label="Conversation mode"
              className={`flex items-center justify-center rounded-md p-1.5 transition ${
                conversation
                  ? "bg-emerald-600/30 text-emerald-300 ring-1 ring-emerald-500/50"
                  : "text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
              }`}
            >
              <ConversationIcon size={16} className={conversation ? "animate-pulse" : undefined} />
            </button>

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

            {(streaming || speaking) && (
              <button
                onClick={() => void stop()}
                data-tooltip={speaking && !streaming ? "Stop speaking" : "Stop the agent"}
                aria-label="Stop"
                className="relative z-10 ml-auto mr-7 flex items-center gap-1 rounded-md bg-red-600/90 px-2 py-1 text-[11px] font-medium text-white transition hover:bg-red-600"
              >
                <StopIcon size={12} /> {speaking && !streaming ? "Hush" : "Stop"}
              </button>
            )}

          </div>

          {/* Context usage (hourglass) — centered on the right edge of the box */}
          <div className="absolute right-2 top-1/2 -translate-y-1/2">

            <AgentContextUsageButton
              usage={contextUsage}
              compactFlipCount={compactFlipCount}
              open={showContextUsage}
              onToggle={() => setShowContextUsage((v) => !v)}
              onClose={() => setShowContextUsage(false)}
              placement="composer"
            />

          </div>

        </div>

      </div>

    </aside>

  );

}


