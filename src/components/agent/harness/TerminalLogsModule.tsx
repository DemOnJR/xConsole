import { useState } from "react";
import { useHarnessStore } from "../../../stores/harnessStore";
import { useAgentStore } from "../../../stores/agentStore";

export function TerminalLogsModule() {
  const showLogs = useHarnessStore((s) => s.showLogs);
  const messages = useAgentStore((s) => s.messages);

  const [filter, setFilter] = useState("");

  if (!showLogs) return null;

  const logLines: string[] = [];
  for (const m of messages) {
    if (m.segments) {
      for (const seg of m.segments) {
        if (seg.type === "text" && seg.content) {
          const lines = seg.content.split("\n").filter(Boolean);
          logLines.push(...lines);
        }
      }
    } else if (m.content) {
      const lines = m.content.split("\n").filter(Boolean);
      logLines.push(...lines);
    }
  }

  const filtered = filter.trim()
    ? logLines.filter((l) => l.toLowerCase().includes(filter.toLowerCase()))
    : logLines;

  return (
    <div className="border-b border-[var(--border)] bg-black/70 text-xs font-mono">
      {/* Header bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[var(--surface-2)]/60 border-b border-[var(--border)]/40">
        <div className="flex items-center gap-2">
          <span className="text-emerald-400 select-none font-semibold">📜</span>
          <span className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider font-semibold">
            TERMINAL LOGS &amp; DIAGNOSTICS ({filtered.length})
          </span>
        </div>

        <input
          type="text"
          className="rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-[10px] text-[var(--text)] placeholder-[var(--text-dim)] focus:outline-none w-36"
          placeholder="Filter logs..."
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>

      {/* Raw log stream */}
      <div className="max-h-40 overflow-y-auto p-2 space-y-0.5 text-[10px] text-gray-300 font-mono select-text">
        {filtered.length === 0 ? (
          <div className="py-2 text-center text-[10px] text-[var(--text-dim)]">
            No diagnostic log events captured yet.
          </div>
        ) : (
          filtered.slice(-40).map((line, idx) => (
            <div key={idx} className="leading-tight break-all">
              <span className="text-[var(--text-dim)] select-none mr-2">›</span>
              {line}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
