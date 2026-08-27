import { useState, useRef, useEffect, memo } from "react";
import { HarnessModuleBar } from "./HarnessModuleBar";
import { GoalModule } from "./GoalModule";
import { ToolCallsModule } from "./ToolCallsModule";
import { ContextMemoryModule } from "./ContextMemoryModule";
import { TerminalLogsModule } from "./TerminalLogsModule";
import type { GoalTaskItem, AgentChatMessage } from "./types";

export interface AgentNodeProps {
  id?: string;
  selected?: boolean;
  onClose?: () => void;
}

export const AgentNodeView = memo(function AgentNodeView({
  id: _id = "agent-node",
  onClose,
}: AgentNodeProps) {
  const [activeModule, setActiveModule] = useState<"goal" | "tools" | "context" | "logs" | null>(null);
  const [goal, setGoal] = useState<string>("Complete current objective and verify system readiness.");
  const [tasks, setTasks] = useState<GoalTaskItem[]>([
    { id: "1", text: "Scan project architecture & verify dependencies", done: true },
    { id: "2", text: "Execute harness test suite & tools verification", done: true },
    { id: "3", text: "Stream response and maintain conversation loop", done: false },
  ]);

  const [messages, setMessages] = useState<AgentChatMessage[]>([
    {
      id: "m1",
      role: "assistant",
      content: "Autonomous Agent Engine initialized. Ready to execute directives.",
      tokens: 389,
    },
  ]);

  const [input, setInput] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [fontSize, setFontSize] = useState(12);
  const [agentMode, setAgentMode] = useState<"auto" | "plan" | "code">("auto");
  const [safetyMode, setSafetyMode] = useState<"autonomous" | "prompt">("autonomous");
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!input.trim() || isStreaming) return;

    const userText = input.trim();
    const userMsg: AgentChatMessage = {
      id: Date.now().toString(),
      role: "user",
      content: userText,
    };

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setIsStreaming(true);

    setTimeout(() => {
      const replyMsg: AgentChatMessage = {
        id: (Date.now() + 1).toString(),
        role: "assistant",
        content: `Executed directive: "${userText}". Workflow completed with 0 errors.`,
        tokens: 142,
      };
      setMessages((prev) => [...prev, replyMsg]);
      setIsStreaming(false);
    }, 600);
  };

  const handleToggleTask = (taskId: string) => {
    setTasks((prev) =>
      prev.map((t) => (t.id === taskId ? { ...t, done: !t.done } : t))
    );
  };

  const handleAddTask = (text: string) => {
    setTasks((prev) => [...prev, { id: Date.now().toString(), text, done: false }]);
  };

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-2xl font-sans"
      style={{ fontSize: `${fontSize}px` }}
    >
      {/* Top Header Bar */}
      <div className="flex h-9 items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-3 text-xs shrink-0 select-none">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <span className="h-2 w-2 rounded-full bg-cyan-400 animate-pulse" />
          <span className="font-mono text-[11px] font-semibold text-gray-200 truncate">
            &gt; {goal || "Autonomous AI Agent Engine"}
          </span>
        </div>

        <div className="flex items-center gap-1 shrink-0 font-mono text-[10px]">
          <span className="rounded bg-zinc-800 text-cyan-400 border border-zinc-700 px-1.5 py-0.5">
            ⚡ trace
          </span>
          <button
            type="button"
            onClick={() => setFontSize((f) => Math.max(10, f - 1))}
            className="rounded px-1.5 py-0.5 text-zinc-400 hover:text-zinc-100 hover:bg-white/5"
          >
            A-
          </button>
          <button
            type="button"
            onClick={() => setFontSize((f) => Math.min(18, f + 1))}
            className="rounded px-1.5 py-0.5 text-zinc-400 hover:text-zinc-100 hover:bg-white/5"
          >
            A+
          </button>
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="rounded px-1.5 py-0.5 text-zinc-400 hover:text-red-400 hover:bg-white/5 ml-1"
            >
              ✕
            </button>
          )}
        </div>
      </div>

      {/* DeepSeek Harness Modular Bar */}
      <HarnessModuleBar
        activeModule={activeModule}
        onToggleModule={(mod) => setActiveModule(activeModule === mod ? null : mod)}
      />

      {/* Conditionally Rendered Sub-Modules */}
      {activeModule === "goal" && (
        <GoalModule
          activeGoal={goal}
          tasks={tasks}
          onSaveGoal={setGoal}
          onToggleTask={handleToggleTask}
          onAddTask={handleAddTask}
        />
      )}
      {activeModule === "tools" && <ToolCallsModule />}
      {activeModule === "context" && <ContextMemoryModule />}
      {activeModule === "logs" && <TerminalLogsModule />}

      {/* Conversation Turns Body */}
      <div className="flex-1 overflow-y-auto p-4 space-y-3 font-mono text-xs">
        {messages.map((m) => (
          <div key={m.id} className="space-y-1.5">
            {m.role === "user" ? (
              <div className="flex items-start gap-2 text-zinc-300">
                <span className="text-cyan-400 font-semibold">~#</span>
                <span className="font-sans text-gray-100">{m.content}</span>
              </div>
            ) : (
              <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3 space-y-2">
                {/* Grouped Tool Actions Badge */}
                <div className="flex items-center justify-between text-[11px] text-zinc-400 border-b border-white/5 pb-1.5">
                  <div className="flex items-center gap-1.5">
                    <span className="h-1.5 w-1.5 rounded-full bg-red-400" />
                    <span>executed 4 commands &bull; read 2 files &bull; wrote 2 files</span>
                  </div>
                  <span className="text-[10px] text-zinc-500">8 actions &gt;</span>
                </div>

                <div className="font-sans text-xs text-gray-200 leading-relaxed whitespace-pre-wrap">
                  {m.content}
                </div>

                {m.tokens && (
                  <div className="text-[10px] text-zinc-500 font-mono pt-1">
                    Worked for 12s &bull; {m.tokens} tok (28.4 t/s)
                  </div>
                )}
              </div>
            )}
          </div>
        ))}
        {isStreaming && (
          <div className="flex items-center gap-2 text-zinc-400 text-xs py-2 font-mono">
            <span className="animate-spin">⠋</span> thinking &amp; executing tools…
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Interactive Input Bar */}
      <div className="border-t border-[var(--border)] bg-[var(--surface-2)] p-3 shrink-0">
        <form onSubmit={handleSend} className="space-y-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
            placeholder="~# Ask anything... (paste an image · / for commands · Enter to send)"
            rows={2}
            className="w-full resize-none rounded-lg border border-[var(--border)] bg-[var(--surface)] p-2 text-xs text-gray-200 placeholder-zinc-500 focus:border-zinc-400 focus:outline-none font-mono"
          />

          <div className="flex items-center justify-between pt-1">
            <div className="flex items-center gap-1.5 text-xs">
              <button
                type="button"
                onClick={() => setSafetyMode(safetyMode === "autonomous" ? "prompt" : "autonomous")}
                className="rounded bg-zinc-800 border border-zinc-700 px-2 py-0.5 text-[10px] font-mono text-zinc-300 hover:text-white"
                title="Safety profile"
              >
                🛡️ {safetyMode}
              </button>
              <button
                type="button"
                onClick={() => setAgentMode(agentMode === "auto" ? "plan" : agentMode === "plan" ? "code" : "auto")}
                className="rounded bg-zinc-800 border border-zinc-700 px-2 py-0.5 text-[10px] font-mono text-zinc-300 hover:text-white"
                title="Runtime mode"
              >
                ⚡ {agentMode}
              </button>
              <button
                type="button"
                className="rounded bg-zinc-800 border border-zinc-700 px-2 py-0.5 text-[10px] font-mono text-zinc-400 hover:text-white"
                title="Attach Vision Image"
              >
                🖼️
              </button>
            </div>

            <div className="flex items-center gap-2 font-mono text-xs">
              <span className="text-[10px] text-zinc-400">26% context</span>
              <button
                type="submit"
                disabled={!input.trim() || isStreaming}
                className="flex h-7 w-7 items-center justify-center rounded-lg bg-zinc-100 text-zinc-950 hover:bg-white disabled:opacity-40 font-bold transition"
              >
                ▶
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
});

export default AgentNodeView;
