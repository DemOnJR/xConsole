import { useState } from "react";
import { HarnessModuleBar } from "./HarnessModuleBar";
import { GoalModule } from "./GoalModule";
import type { GoalTaskItem, AgentChatMessage } from "./types";

export function AgentNode() {
  const [activeGoal, setActiveGoal] = useState("Optimize system performance and configure backups");
  const [tasks, setTasks] = useState<GoalTaskItem[]>([
    { id: "t1", text: "Audit resource utilization across active nodes", done: true },
    { id: "t2", text: "Configure automated database backup cron job", done: false },
  ]);
  const [messages, setMessages] = useState<AgentChatMessage[]>([
    { id: "m1", role: "assistant", content: "Autonomous Agent Engine initialized. Ready to execute directives." },
  ]);
  const [input, setInput] = useState("");

  const [showGoal, setShowGoal] = useState(true);
  const [showTools, setShowTools] = useState(true);
  const [showContext, setShowContext] = useState(true);
  const [showLogs, setShowLogs] = useState(false);

  const handleSend = () => {
    if (!input.trim()) return;
    const userMsg: AgentChatMessage = { id: `m-${Date.now()}`, role: "user", content: input };
    setMessages((prev) => [...prev, userMsg]);
    setInput("");

    setTimeout(() => {
      setMessages((prev) => [
        ...prev,
        { id: `m-reply-${Date.now()}`, role: "assistant", content: `Received directive: "${userMsg.content}". Executing workflow...` },
      ]);
    }, 600);
  };

  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--bg)] shadow-lg font-mono text-xs">
      <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 font-medium">
        <div className="flex items-center gap-2">
          <span className="text-cyan-400">●</span>
          <span className="text-gray-200">Autonomous AI Agent Engine</span>
        </div>
      </div>

      <HarnessModuleBar
        showGoal={showGoal}
        showTools={showTools}
        showContext={showContext}
        showLogs={showLogs}
        onToggle={(m) => {
          if (m === "goal") setShowGoal(!showGoal);
          if (m === "tools") setShowTools(!showTools);
          if (m === "context") setShowContext(!showContext);
          if (m === "logs") setShowLogs(!showLogs);
        }}
      />

      {showGoal && (
        <GoalModule
          activeGoal={activeGoal}
          tasks={tasks}
          onSaveGoal={setActiveGoal}
          onToggleTask={(tid) => setTasks((ts) => ts.map((t) => (t.id === tid ? { ...t, done: !t.done } : t)))}
          onAddTask={(text) => setTasks((ts) => [...ts, { id: `t-${Date.now()}`, text, done: false }])}
        />
      )}

      {/* Messages Stream */}
      <div className="flex-1 overflow-y-auto p-3 space-y-2">
        {messages.map((m) => (
          <div
            key={m.id}
            className={`rounded p-2 text-xs leading-relaxed ${
              m.role === "user"
                ? "bg-[var(--surface-2)] text-[var(--text)] ml-6"
                : "bg-cyan-950/20 border border-cyan-500/20 text-cyan-200 mr-6"
            }`}
          >
            <span className="text-[10px] text-[var(--text-dim)] uppercase block mb-1">
              {m.role === "user" ? "You" : "Agent"}
            </span>
            {m.content}
          </div>
        ))}
      </div>

      {/* Input Composer */}
      <div className="border-t border-[var(--border)] bg-[var(--surface)] p-2 flex gap-2">
        <input
          type="text"
          className="flex-1 rounded border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:outline-none focus:border-cyan-400 font-mono"
          placeholder="Send directive or /command to Agent..."
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSend()}
        />
        <button
          className="rounded bg-cyan-600 px-3 py-1.5 text-xs text-white hover:bg-cyan-500 font-medium"
          onClick={handleSend}
        >
          Send
        </button>
      </div>
    </div>
  );
}
