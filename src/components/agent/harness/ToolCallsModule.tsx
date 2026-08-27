import { useState } from "react";
import { useHarnessStore } from "../../../stores/harnessStore";
import { useAgentStore } from "../../../stores/agentStore";

export function ToolCallsModule() {
  const showTools = useHarnessStore((s) => s.showTools);
  const toolsCollapsed = useHarnessStore((s) => s.toolsCollapsed);
  const toggleToolsCollapsed = useHarnessStore((s) => s.toggleToolsCollapsed);

  const messages = useAgentStore((s) => s.messages);
  const isStreaming = useAgentStore((s) => s.streaming);

  const [expandedCalls, setExpandedCalls] = useState<Record<string, boolean>>({});

  if (!showTools) return null;

  const recentActivities: Array<{
    id: string;
    toolName: string;
    status: "running" | "success" | "error";
    detail?: string;
  }> = [];

  for (const m of messages) {
    if (m.activity) {
      for (const act of m.activity) {
        recentActivities.push({
          id: act.id,
          toolName: act.label || act.tool || act.kind || "tool_call",
          status: act.state === "running" ? "running" : act.state === "error" ? "error" : "success",
          detail: act.detail || act.output,
        });
      }
    }
  }

  const toggleCall = (id: string) => {
    setExpandedCalls((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  return (
    <div className="border-b border-[var(--border)] bg-[var(--surface)]/80 text-xs font-mono">
      {/* Header bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[var(--surface-2)]/40">
        <div className="flex items-center gap-2">
          <span className="text-cyan-400 select-none font-semibold">⚡</span>
          <span className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider font-semibold">
            TOOL EXECUTIONS ({recentActivities.length})
          </span>
          {isStreaming && (
            <span className="flex items-center gap-1 text-[10px] text-cyan-300 font-mono">
              <span className="animate-spin text-[10px]">⠋</span> live
            </span>
          )}
        </div>

        <button
          type="button"
          className="text-[var(--text-dim)] hover:text-[var(--text)] p-1 rounded hover:bg-[var(--surface-2)] transition"
          onClick={toggleToolsCollapsed}
          title={toolsCollapsed ? "Expand Tools view" : "Collapse Tools view"}
        >
          {toolsCollapsed ? "▼" : "▲"}
        </button>
      </div>

      {/* Expanded Tools Stream */}
      {!toolsCollapsed && (
        <div className="max-h-48 overflow-y-auto p-2 space-y-1.5 divide-y divide-[var(--border)]/20">
          {recentActivities.length === 0 ? (
            <div className="py-2 text-center text-[11px] text-[var(--text-dim)]">
              No tool executions in current session yet.
            </div>
          ) : (
            recentActivities.slice(-8).reverse().map((act) => {
              const isExpanded = Boolean(expandedCalls[act.id]);
              return (
                <div key={act.id} className="pt-1.5 first:pt-0">
                  <div
                    className="flex items-center justify-between gap-2 cursor-pointer hover:bg-[var(--surface-2)]/50 rounded px-1.5 py-1 transition"
                    onClick={() => toggleCall(act.id)}
                  >
                    <div className="flex items-center gap-2 truncate flex-1 min-w-0">
                      <span
                        className={`text-[10px] font-mono px-1 py-0.2 rounded border uppercase ${
                          act.status === "running"
                            ? "bg-cyan-500/10 text-cyan-300 border-cyan-500/30 animate-pulse"
                            : act.status === "error"
                              ? "bg-red-500/10 text-red-300 border-red-500/30"
                              : "bg-emerald-500/10 text-emerald-300 border-emerald-500/30"
                        }`}
                      >
                        {act.status === "running" ? "⠋ run" : act.status === "error" ? "✕ err" : "✓ ok"}
                      </span>
                      <span className="font-semibold text-[11px] text-[var(--text)] truncate">
                        {act.toolName}
                      </span>
                    </div>

                    <span className="text-[10px] text-[var(--text-dim)]">
                      {isExpanded ? "hide output ▲" : "view output ▼"}
                    </span>
                  </div>

                  {isExpanded && act.detail && (
                    <div className="mt-1 rounded border border-[var(--border)] bg-black/40 p-2 text-[10px] text-[var(--text)] font-mono overflow-x-auto max-h-28">
                      <pre className="whitespace-pre-wrap">{act.detail}</pre>
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
