import type { AgentChatMessage } from "../../stores/agentStore";
import { AgentMarkdown } from "./AgentMarkdown";
import { AgentTokenStats } from "./AgentTokenStats";
import { consoleRows } from "./agentConsoleRows";
import type { PrefixTelemetry, TurnTelemetry, TokenStats } from "../../lib/streamStats";

function stateMark(state: "running" | "done" | "error") {
  if (state === "running") return "…";
  if (state === "error") return "!";
  return "✓";
}

export function AgentConsole({
  messages,
  streamingText,
  streaming,
  streamStats,
  turnTelemetry,
  prefixTelemetry,
  expanded,
}: {
  messages: AgentChatMessage[];
  streamingText: string;
  streaming: boolean;
  streamStats: TokenStats | null;
  turnTelemetry: TurnTelemetry | null;
  prefixTelemetry: PrefixTelemetry | null;
  expanded: boolean;
}) {
  const rows = consoleRows(messages);
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[var(--bg)] font-mono">
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-3 text-[11px] leading-relaxed">
        {rows.map((row, index) => {
          if (row.kind === "user") {
            return (
              <div key={index} className="flex gap-2 text-[var(--text)]">
                <span className="shrink-0 text-cyan-400">›</span>
                <div className="min-w-0 flex-1 font-sans text-sm">
                  <AgentMarkdown content={row.content} variant="user" />
                </div>
              </div>
            );
          }
          if (row.kind === "assistant") {
            return (
              <div key={index} className="flex gap-2 text-[var(--text)]">
                <span className="shrink-0 text-emerald-400">•</span>
                <div className={`min-w-0 ${expanded ? "w-full" : "w-[92%]"}`}>
                  <AgentMarkdown content={row.content} variant="assistant" />
                </div>
              </div>
            );
          }
          if (row.kind === "edit") {
            return (
              <div key={index} className="flex items-center gap-2 text-[var(--text-faint)]">
                <span className={row.state === "error" ? "text-red-400" : "text-amber-400"}>
                  {stateMark(row.state)}
                </span>
                <span>{row.label}</span>
                <span className="text-[10px] text-gray-600">
                  +{row.added}/-{row.removed}
                </span>
              </div>
            );
          }
          return (
            <div key={index} className="flex items-center gap-2 text-[var(--text-faint)]">
              <span className={row.state === "error" ? "text-red-400" : "text-cyan-500"}>
                {stateMark(row.state)}
              </span>
              <span>{row.label}</span>
            </div>
          );
        })}

        {streaming && (
          <div className="flex gap-2 text-[var(--text)]">
            <span className="shrink-0 text-emerald-400">•</span>
            <div className={`min-w-0 ${expanded ? "w-full" : "w-[92%]"}`}>
              {streamingText ? (
                <AgentMarkdown content={streamingText} variant="assistant" />
              ) : (
                <span className="text-gray-500">Thinking…</span>
              )}
              {streamStats && (
                <div className="mt-1">
                  <AgentTokenStats stats={streamStats} telemetry={turnTelemetry} live />
                </div>
              )}
              {prefixTelemetry && prefixTelemetry.classification !== "append_only" ? (
                <div className="mt-1 text-[9px] text-gray-600">
                  prefix {prefixTelemetry.classification} · {prefixTelemetry.source}
                </div>
              ) : null}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
