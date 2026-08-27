import { useHarnessStore } from "../../../../src/stores/harnessStore";
import { useAgentStore } from "../../../../src/stores/agentStore";
import {
  TargetIcon,
  ToolsIcon,
  BrainIcon,
  TerminalIcon,
  SequentialDots,
} from "../../../../src/components/icons";

export function HarnessModuleBar() {
  const showGoal = useHarnessStore((s) => s.showGoal);
  const showTools = useHarnessStore((s) => s.showTools);
  const showContext = useHarnessStore((s) => s.showContext);
  const showLogs = useHarnessStore((s) => s.showLogs);
  const toggleModule = useHarnessStore((s) => s.toggleModule);
  const activeGoal = useHarnessStore((s) => s.activeGoal);
  const goalStatus = useHarnessStore((s) => s.goalStatus);
  const isStreaming = useAgentStore((s) => s.streaming);

  return (
    <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)]/60 px-3 py-1 text-xs font-mono select-none">
      {/* Module Toggles */}
      <div className="flex items-center gap-1.5 overflow-x-auto">
        <span className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider mr-1">
          Modules:
        </span>

        {/* Goal Module Toggle */}
        <button
          type="button"
          className={`flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] transition border ${
            showGoal
              ? "bg-[var(--surface)] text-amber-300 border-amber-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => toggleModule("goal")}
          title="Toggle Goal & Objective Module"
        >
          <TargetIcon size={12} className="text-amber-400 shrink-0" />
          <span>goal</span>
          {activeGoal && (
            <span
              className={`h-1.5 w-1.5 rounded-full shrink-0 ${
                goalStatus === "running" ? "bg-amber-400 animate-pulse" : "bg-emerald-400"
              }`}
            />
          )}
        </button>

        {/* Tools Module Toggle */}
        <button
          type="button"
          className={`flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] transition border ${
            showTools
              ? "bg-[var(--surface)] text-cyan-300 border-cyan-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => toggleModule("tools")}
          title="Toggle Tool Executions Stream"
        >
          <ToolsIcon size={12} className="text-cyan-400 shrink-0" />
          <span>tools</span>
          {isStreaming && (
            <span className="h-1.5 w-1.5 rounded-full bg-cyan-400 shrink-0 animate-pulse" />
          )}
        </button>

        {/* Context & Memory Module Toggle */}
        <button
          type="button"
          className={`flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] transition border ${
            showContext
              ? "bg-[var(--surface)] text-violet-300 border-violet-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => toggleModule("context")}
          title="Toggle Token Budget & Context Memory"
        >
          <BrainIcon size={12} className="text-violet-400 shrink-0" />
          <span>context</span>
        </button>

        {/* Logs Module Toggle */}
        <button
          type="button"
          className={`flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] transition border ${
            showLogs
              ? "bg-[var(--surface)] text-emerald-300 border-emerald-400/30 shadow-xs"
              : "bg-transparent text-[var(--text-dim)] border-transparent hover:border-[var(--border)] hover:text-[var(--text)]"
          }`}
          onClick={() => toggleModule("logs")}
          title="Toggle Terminal Stderr/Stdout Logs"
        >
          <TerminalIcon size={12} className="text-emerald-400 shrink-0" />
          <span>logs</span>
        </button>
      </div>

      {/* Terminal Aesthetic Status Indicator */}
      <div className="flex items-center gap-2 text-[11px] text-[var(--text-dim)]">
        {isStreaming ? (
          <span className="flex items-center gap-1.5 text-cyan-400 font-medium">
            <SequentialDots size={16} className="text-cyan-400" />
            <span>working</span>
          </span>
        ) : (
          <span className="text-[10px] text-[var(--text-dim)]">
            harness: <span className="text-emerald-400">ready</span>
          </span>
        )}
      </div>
    </div>
  );
}
