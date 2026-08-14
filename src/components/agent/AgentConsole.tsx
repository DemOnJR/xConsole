import { useEffect, useRef, useState } from "react";
import type { AgentChatMessage, TurnSegment } from "../../stores/agentStore";
import { plainText } from "../../lib/plainText";
import { AgentMarkdown } from "./AgentMarkdown";
import { AgentActivityFeed, AgentThinking } from "./AgentActivity";
import { segmentsFromMessage } from "../../stores/turnSegments";
import { previewSrc } from "../../lib/vision";

function AssistantTurn({
  segments,
  live = false,
  expanded,
  executeTarget,
  onExecute,
}: {
  segments: TurnSegment[];
  live?: boolean;
  expanded: boolean;
  executeTarget?: { name: string; host: string } | null;
  onExecute?: (code: string) => void;
}) {
  const lastActivity = live
    ? [...segments].reverse().findIndex((s) => s.type === "activity")
    : -1;
  const lastActivityIdx = lastActivity >= 0 ? segments.length - 1 - lastActivity : -1;

  if (live && segments.length === 0) {
    return <AgentThinking />;
  }

  return (
    <div className="flex flex-col gap-2">
      {segments.map((seg, i) => {
        if (seg.type === "text") {
          if (!seg.content.trim()) return null;
          return (
            <div key={`text-${i}`} className="flex gap-2 text-[var(--text)]">
              <span className="shrink-0 text-emerald-400">•</span>
              <div className={`min-w-0 ${expanded ? "w-full" : "w-[92%]"}`}>
                {live ? (
                  <div className="whitespace-pre-wrap break-words">{plainText(seg.content)}</div>
                ) : (
                  <AgentMarkdown
                    content={seg.content}
                    variant="assistant"
                    executeTarget={executeTarget}
                    onExecute={onExecute}
                  />
                )}
              </div>
            </div>
          );
        }
        return (
          <div key={`act-${i}`} className="pl-4">
            <AgentActivityFeed items={seg.items} live={live && i === lastActivityIdx} />
          </div>
        );
      })}
    </div>
  );
}

export function AgentConsole({
  messages,
  streamingSegments = [],
  streaming,
  expanded,
  executeTarget,
  onExecute,
  fontSize = 11,
}: {
  messages: AgentChatMessage[];
  /** Live turn timeline (text / tools / more text), in order. */
  streamingSegments?: TurnSegment[];
  streaming: boolean;
  expanded: boolean;
  executeTarget?: { name: string; host: string } | null;
  onExecute?: (code: string) => void;
  /** Console font size in px (A−/A+ in the status line). */
  fontSize?: number;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [userScrolledUp, setUserScrolledUp] = useState(false);

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
  }, [messages.length, streamingSegments, streaming, userScrolledUp]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col bg-[var(--bg)] font-mono">
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        style={{ fontSize }}
        className="nowheel flex min-h-0 flex-1 cursor-text select-text flex-col gap-2 overflow-y-auto px-3 py-3 leading-relaxed"
      >
        {messages.map((message, index) => {
          // Stable-ish key: role+index keeps CommandCard state attached across
          // re-renders (compaction markers can shift plain indices).
          const key = message.isCompaction ? `compact-${index}` : `${message.role}-${index}`;
          if (message.isCompaction) {
            return (
              <div
                key={key}
                className="my-1.5 flex items-center justify-between gap-2 rounded border border-cyan-500/25 bg-cyan-950/20 px-2.5 py-1.5 text-[11px]"
              >
                <div className="flex items-center gap-2 text-cyan-300">
                  <span className="text-amber-400">⚡</span>
                  <span className="font-semibold">
                    {message.content || "Context compacted"}
                  </span>
                  {message.compactionTokensBefore && message.compactionTokensAfter ? (
                    <span className="text-cyan-400/80">
                      (~{message.compactionTokensBefore.toLocaleString()} → ~
                      {message.compactionTokensAfter.toLocaleString()} tokens)
                    </span>
                  ) : null}
                </div>
                {message.compactionPrunedTools ? (
                  <span className="text-[10px] text-cyan-400/60">
                    {message.compactionPrunedTools} tool output
                    {message.compactionPrunedTools > 1 ? "s" : ""} pruned
                  </span>
                ) : null}
              </div>
            );
          }

          if (message.role === "user") {
            return (
              <div key={key} className="flex gap-2 text-[var(--text)]">
                <span className="shrink-0 font-bold text-cyan-400">~#</span>
                <div className="min-w-0 flex-1">
                  <div className="whitespace-pre-wrap break-words">{plainText(message.content)}</div>
                  {message.images && message.images.length > 0 ? (
                    <div className="mt-1.5 flex flex-wrap gap-1.5">
                      {message.images.map((img, i) => (
                        <img
                          key={`${img.name ?? "img"}-${i}`}
                          src={previewSrc(img)}
                          alt={img.name || `Image #${i + 1}`}
                          className="h-16 max-w-[128px] rounded border border-[var(--border)] object-cover"
                        />
                      ))}
                    </div>
                  ) : null}
                </div>
              </div>
            );
          }

          if (message.role === "assistant") {
            return (
              <AssistantTurn
                key={key}
                segments={segmentsFromMessage(message)}
                expanded={expanded}
                executeTarget={executeTarget}
                onExecute={onExecute}
              />
            );
          }

          return null;
        })}

        {streaming && (
          <AssistantTurn
            segments={streamingSegments}
            live
            expanded={expanded}
            executeTarget={executeTarget}
            onExecute={onExecute}
          />
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
