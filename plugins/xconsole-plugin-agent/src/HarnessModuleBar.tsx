export function HarnessModuleBar({
  activeModule,
  onToggleModule,
  showGoal,
  showTools,
  showContext,
  showLogs,
  onToggle,
}: {
  activeModule?: "goal" | "tools" | "context" | "logs" | null;
  onToggleModule?: (module: "goal" | "tools" | "context" | "logs") => void;
  showGoal?: boolean;
  showTools?: boolean;
  showContext?: boolean;
  showLogs?: boolean;
  onToggle?: (module: "goal" | "tools" | "context" | "logs") => void;
}) {
  const isGoal = activeModule ? activeModule === "goal" : Boolean(showGoal);
  const isTools = activeModule ? activeModule === "tools" : Boolean(showTools);
  const isContext = activeModule ? activeModule === "context" : Boolean(showContext);
  const isLogs = activeModule ? activeModule === "logs" : Boolean(showLogs);

  const handleToggle = (mod: "goal" | "tools" | "context" | "logs") => {
    if (onToggleModule) onToggleModule(mod);
    if (onToggle) onToggle(mod);
  };

  return (
    <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)]/60 px-3 py-1 text-xs font-mono select-none">
      <div className="flex items-center gap-1.5 overflow-x-auto">
        <span className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider mr-1">Modules:</span>

        <button
          type="button"
          className={`flex items-center gap-1 rounded px-2 py-0.5 text-[11px] border transition ${
            isGoal
              ? "bg-[var(--surface)] text-amber-300 border-amber-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => handleToggle("goal")}
        >
          <span>🎯</span>
          <span>goal</span>
        </button>

        <button
          type="button"
          className={`flex items-center gap-1 rounded px-2 py-0.5 text-[11px] border transition ${
            isTools
              ? "bg-[var(--surface)] text-cyan-300 border-cyan-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => handleToggle("tools")}
        >
          <span>⚡</span>
          <span>tools</span>
        </button>

        <button
          type="button"
          className={`flex items-center gap-1 rounded px-2 py-0.5 text-[11px] border transition ${
            isContext
              ? "bg-[var(--surface)] text-violet-300 border-violet-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => handleToggle("context")}
        >
          <span>🧠</span>
          <span>context</span>
        </button>

        <button
          type="button"
          className={`flex items-center gap-1 rounded px-2 py-0.5 text-[11px] border transition ${
            isLogs
              ? "bg-[var(--surface)] text-emerald-300 border-emerald-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => handleToggle("logs")}
        >
          <span>📜</span>
          <span>logs</span>
        </button>
      </div>

      <div className="flex items-center gap-2 text-[10px] text-[var(--text-dim)]">
        harness: <span className="text-emerald-400">ready</span>
      </div>
    </div>
  );
}

