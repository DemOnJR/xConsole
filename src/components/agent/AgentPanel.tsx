import { useEffect, useMemo, useRef, useState } from "react";

import { useAgentStore } from "../../stores/agentStore";

import { useUiStore } from "../../stores/uiStore";

import { useVpsStore } from "../../stores/vpsStore";

import { useSettingsStore } from "../../stores/settingsStore";

import { BotIcon, MaximizeIcon, MinimizeIcon, SettingsIcon, TrashIcon } from "../icons";

import { AgentMarkdown } from "./AgentMarkdown";

import { AgentActivityFeed, AgentThinking } from "./AgentActivity";
import { AgentTokenStats } from "./AgentTokenStats";
import { AgentContextUsageButton } from "./AgentContextUsage";

import { AgentHistory } from "./AgentHistory";

import type { AgentChatMessage } from "../../stores/agentStore";



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

              : "border border-[#1f2737] bg-[#11161f] text-gray-200"

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



const CLI_KINDS = new Set(["cursor", "codex_cli", "opencode_cli"]);

const TOOL_KINDS = new Set(["openai", "anthropic", "ollama"]);

const CURSOR_KIND = "cursor";



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

    error,

    targets,

    pendingApprovals,

    send,

    setTargets,

    init,

    newConversation,

    openConversation,

    removeConversation,

    resolveApproval,

  } = useAgentStore();



  const vpsList = useVpsStore((s) => s.vpsList);

  const loadVps = useVpsStore((s) => s.load);

  const loadSettings = useSettingsStore((s) => s.load);

  const providers = useSettingsStore((s) => s.providers);

  const activeProviderId = useSettingsStore((s) => s.settings["agent.active_provider"]);



  const [input, setInput] = useState("");

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

  };



  const toggleTarget = (id: string) =>

    setTargets(

      targets.includes(id) ? targets.filter((t) => t !== id) : [...targets, id],

    );



  if (!open && !expanded) return null;



  return (

    <aside

      className={`flex shrink-0 flex-col border-[#1f2737] bg-[#0d121b] ${

        expanded

          ? "h-full min-w-0 flex-1 border-0"

          : "h-full w-[420px] border-l"

      }`}

    >

      <div className="flex items-center gap-2 border-b border-[#1f2737] px-3 py-2.5">

        <BotIcon size={16} />

        <span className="text-xs font-medium uppercase tracking-wider text-gray-300">

          Agent

        </span>

        <span className="truncate text-[11px] text-gray-500">

          {activeProvider ? `${activeProvider.name}` : "no provider"}

        </span>

        <div className="ml-auto flex items-center gap-1">

          <button

            className="rounded-md px-1.5 py-1 text-[10px] text-gray-400 hover:bg-[#1f2737] hover:text-gray-200"

            title="Chat history"

            onClick={() => setShowHistory((v) => !v)}

          >

            History

          </button>

          <button

            className="rounded-md p-1 text-gray-400 hover:bg-[#1f2737] hover:text-gray-200"

            title="New conversation"

            onClick={() => void newConversation()}

          >

            <TrashIcon size={15} />

          </button>

          <button

            className="rounded-md p-1 text-gray-400 hover:bg-[#1f2737] hover:text-gray-200"

            title="Agent settings"

            onClick={() => openSettings("agent")}

          >

            <SettingsIcon size={15} />

          </button>

          <button

            className="rounded-md p-1 text-gray-400 hover:bg-[#1f2737] hover:text-gray-200"

            title={expanded ? "Exit fullscreen" : "Expand to full window"}

            onClick={() => expanded ? setAgentExpanded(false) : toggleAgentExpanded()}

          >

            {expanded ? <MinimizeIcon size={15} /> : <MaximizeIcon size={15} />}

          </button>

          {!expanded && (

            <button

              className="rounded-md p-1 text-gray-400 hover:bg-[#1f2737] hover:text-gray-200"

              title="Close"

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

      <div className="border-b border-[#1f2737] px-3 py-1.5">

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

              className="rounded-full border border-[#1f2737] px-2 py-0.5 text-[10px] text-gray-300 hover:bg-[#1f2737]"

            >

              All

            </button>

            <button

              onClick={() => setTargets([])}

              className="rounded-full border border-[#1f2737] px-2 py-0.5 text-[10px] text-gray-300 hover:bg-[#1f2737]"

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

                    : "border-[#1f2737] text-gray-400 hover:bg-[#1f2737]"

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



      {/* Pending approvals */}

      {pendingApprovals.length > 0 && (

        <div className="border-t border-[#1f2737] bg-[#0b0f17] px-3 py-2">

          {pendingApprovals.map((a) => (

            <div

              key={a.id}

              className="mb-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 last:mb-0"

            >

              <div className="mb-1 text-[11px] font-medium text-amber-200">

                Approve command?

              </div>

              <pre className="mb-2 max-h-24 overflow-auto whitespace-pre-wrap rounded bg-[#0b0f17] px-2 py-1 font-mono text-[11px] text-gray-300">

                {a.command}

              </pre>

              <div className="flex justify-end gap-2">

                <button

                  onClick={() => resolveApproval(a.id, false)}

                  className="rounded-md border border-[#1f2737] px-2.5 py-1 text-[11px] text-gray-300 hover:bg-[#1f2737]"

                >

                  Deny

                </button>

                <button

                  onClick={() => resolveApproval(a.id, true)}

                  className="rounded-md bg-blue-600 px-2.5 py-1 text-[11px] text-white hover:bg-blue-500"

                >

                  Approve & run

                </button>

              </div>

            </div>

          ))}

        </div>

      )}



      {/* Composer */}

      <div className="border-t border-[#1f2737] p-3">

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

        <div className="relative rounded-xl border border-[#2a3344] bg-[#11161f] shadow-[inset_0_1px_0_rgba(255,255,255,0.03)] focus-within:border-[#3d4a61]">

          <textarea

            ref={inputRef}

            value={input}

            onChange={(e) => setInput(e.target.value)}

            onKeyDown={(e) => {

              if (e.key === "Enter" && !e.shiftKey) {

                e.preventDefault();

                submit();

              }

            }}

            rows={1}

            placeholder="Ask anything… (Enter to send)"

            disabled={streaming}

            className="block w-full resize-none border-0 bg-transparent px-3.5 pb-12 pt-3 text-[13px] leading-relaxed text-gray-200 outline-none placeholder:text-gray-600 disabled:opacity-50"

            style={{ minHeight: "44px", maxHeight: "160px" }}

          />

          <div className="absolute bottom-2 right-2">

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


