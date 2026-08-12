import { useEffect, useRef, useState } from "react";
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
  const scrollRef = useRef<HTMLDivElement>(null);
  const [userScrolledUp, setUserScrolledUp] = useState(false);
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);

  const rows = consoleRows(messages);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= 40;
    setUserScrolledUp(!atBottom);
  };

  const scrollToBottom = () => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    setUserScrolledUp(false);
  };

  useEffect(() => {
    if (!userScrolledUp && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [rows.length, streamingText, userScrolledUp]);

  const copyContent = (text: string, index: number) => {
    void navigator.clipboard.writeText(text);
    setCopiedIndex(index);
    setTimeout(() => setCopiedIndex(null), 2000);
  };

  return (
    <div className="relative flex min-h-0 flex-1 flex-col bg-[var(--bg)] font-mono">
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-3 text-[11px] leading-relaxed"
      >
        {rows.map((row, index) => {
          if (row.kind === "compaction") {
            return (
              <div
                key={index}
                className="my-1.5 flex items-center justify-between gap-2 rounded border border-cyan-500/25 bg-cyan-950/20 px-2.5 py-1.5 text-[11px]"
              >
                <div className="flex items-center gap-2 text-cyan-300">
                  <span className="text-amber-400">⚡</span>
                  <span className="font-semibold">{row.label}</span>
                  {row.tokensBefore && row.tokensAfter ? (
                    <span className="text-cyan-400/80">
                      (~{row.tokensBefore.toLocaleString()} → ~{row.tokensAfter.toLocaleString()} tokens)
                    </span>
                  ) : null}
                </div>
                {row.prunedTools ? (
                  <span className="text-[10px] text-cyan-400/60">
                    {row.prunedTools} tool output{row.prunedTools > 1 ? "s" : ""} pruned
                  </span>
                ) : null}
              </div>
            );
          }

          if (row.kind === "user") {
            return (
              <div key={index} className="flex gap-2 text-[var(--text)]">
                <span className="shrink-0 font-bold text-cyan-400">›</span>
                <div className="min-w-0 flex-1 font-sans text-sm">
                  <AgentMarkdown content={row.content} variant="user" />
                </div>
              </div>
            );
          }

          if (row.kind === "assistant") {
            return (
              <div key={index} className="group relative flex gap-2 text-[var(--text)]">
                <span className="shrink-0 text-emerald-400">•</span>
                <div className={`min-w-0 ${expanded ? "w-full" : "w-[92%]"}`}>
                  <AgentMarkdown content={row.content} variant="assistant" />
                  <button
                    type="button"
                    onClick={() => copyContent(row.content, index)}
                    className="mt-1 hidden rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-gray-400 transition hover:text-gray-200 group-hover:inline-block"
                  >
                    {copiedIndex === index ? "Copied ✓" : "Copy"}
                  </button>
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

      {userScrolledUp && (
        <button
          type="button"
          onClick={scrollToBottom}
          className="absolute bottom-3 right-4 flex items-center gap-1.5 rounded-full border border-[var(--border-strong)] bg-[var(--surface)] px-2.5 py-1 text-[11px] text-cyan-300 shadow-md transition hover:bg-[var(--border)]"
        >
          <span>↓</span>
          <span>Jump to bottom</span>
          {streaming && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-400" />}
        </button>
      )}
    </div>
  );
}

