import { memo, useEffect, useMemo, useRef, useState } from "react";
import { useAgentStore, type AgentActivityItem } from "../../../src/stores/agentStore";
import { segmentsFromMessage } from "../../../src/stores/turnSegments";
import {
  formatDuration,
  isCommandItem,
  isFileEditItem,
  isFileReadItem,
  isSearchItem,
} from "./AgentActivity";
import { useMaskHost } from "../../../src/lib/privacy";
import { ChevronDownIcon, ChevronRightIcon, ClockIcon, CloseIcon, ICON, ToolsIcon } from "../../../src/components/icons";

export const TrajectoryModal = memo(function TrajectoryModal({
  onClose,
}: {
  onClose: () => void;
}) {
  const messages = useAgentStore((s) => s.messages);
  const streamStats = useAgentStore((s) => s.streamStats);
  const conversationCostUsd = useAgentStore((s) => s.conversationCostUsd);
  const maskHost = useMaskHost();

  const [filter, setFilter] = useState<"all" | "tools" | "think" | "messages">("all");
  const [search, setSearch] = useState("");
  const [expandedIndices, setExpandedIndices] = useState<Set<number>>(() => new Set());
  const [copied, setCopied] = useState(false);

  const listRef = useRef<HTMLDivElement | null>(null);

  // Close on Escape
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [onClose]);

  // Non-passive wheel handler: prevents ReactFlow / canvas from intercepting or swallowing wheel scroll
  useEffect(() => {
    const listEl = listRef.current;
    if (!listEl) return;

    const onWheel = (e: WheelEvent) => {
      e.stopPropagation();

      // Check if cursor is over an inner scrollable <pre>
      const target = e.target as HTMLElement | null;
      const innerPre = target?.closest<HTMLElement>("pre");

      if (innerPre && innerPre !== listEl && innerPre.scrollHeight > innerPre.clientHeight) {
        const canScrollInner =
          (e.deltaY > 0 && innerPre.scrollTop + innerPre.clientHeight < innerPre.scrollHeight) ||
          (e.deltaY < 0 && innerPre.scrollTop > 0);
        if (canScrollInner) {
          innerPre.scrollTop += e.deltaY;
          e.preventDefault();
          return;
        }
      }

      // Scroll the main list
      const canScrollList =
        (e.deltaY > 0 && listEl.scrollTop + listEl.clientHeight < listEl.scrollHeight) ||
        (e.deltaY < 0 && listEl.scrollTop > 0);

      if (canScrollList || listEl.scrollHeight > listEl.clientHeight) {
        listEl.scrollTop += e.deltaY;
        e.preventDefault();
      }
    };

    listEl.addEventListener("wheel", onWheel, { capture: true, passive: false });
    return () => {
      listEl.removeEventListener("wheel", onWheel, true);
    };
  }, [expandedIndices]);

  // Parse chronological trajectory events from all conversation messages
  const events = useMemo(() => {
    const list: Array<{
      id: string;
      index: number;
      turnIndex: number;
      role: "user" | "assistant" | "system";
      kind: "user_message" | "assistant_message" | "tool_call" | "reasoning" | "status";
      title: string;
      subtitle?: string;
      content?: string;
      durationFormatted?: string;
      tokenStats?: import("../../../src/stores/agentStore").TokenStats;
      activityItem?: AgentActivityItem;
    }> = [];

    let eventIdx = 0;
    messages.forEach((msg, turnIdx) => {
      if (msg.role === "user") {
        list.push({
          id: `msg-${turnIdx}`,
          index: eventIdx++,
          turnIndex: turnIdx,
          role: "user",
          kind: "user_message",
          title: "User Prompt",
          content: msg.content,
        });
      } else if (msg.role === "assistant") {
        const segments = segmentsFromMessage(msg);
        for (const seg of segments) {
          if (seg.type === "activity") {
            for (const item of seg.items) {
              const isCmd = isCommandItem(item);
              const isEdit = isFileEditItem(item);
              const isSearch = isSearchItem(item);
              const isRead = isFileReadItem(item);

              let title = item.label;
              if (isCmd) title = `Command: ${item.label}`;
              else if (isEdit) title = `File Edit: ${item.path || item.label}`;
              else if (isSearch) title = `Grep / Search: ${item.label}`;
              else if (isRead) title = `File Read: ${item.label}`;

              // The outcome belongs in the replay. Without it a tool that failed and a
              // tool that succeeded are the same row, and a trajectory read back to work
              // out what went wrong shows every step as if it had worked.
              const outcome: string[] = [];
              if (item.state === "error") outcome.push("failed");
              else if (item.state === "awaiting_approval") outcome.push("waited for approval");
              if (item.exitCode !== undefined && item.exitCode !== 0) {
                outcome.push(`exit ${item.exitCode}`);
              }
              if (item.truncated) outcome.push("output truncated");
              const duration = formatDuration(item.durationMs);
              if (duration) outcome.push(duration);

              list.push({
                id: `act-${item.id}-${eventIdx}`,
                index: eventIdx++,
                turnIndex: turnIdx,
                role: "assistant",
                kind: "tool_call",
                title,
                subtitle: [item.tool ? `tool: ${item.tool}` : "", outcome.join(" · ")]
                  .filter(Boolean)
                  .join("  ·  ") || undefined,
                content: item.output || item.detail,
                activityItem: item,
              });
            }
          } else if (seg.type === "text") {
            list.push({
              id: `text-${turnIdx}-${eventIdx}`,
              index: eventIdx++,
              turnIndex: turnIdx,
              role: "assistant",
              kind: "assistant_message",
              title: "Assistant Response",
              content: seg.content,
              durationFormatted: msg.durationFormatted,
              tokenStats: msg.tokenStats,
            });
          }
        }
      }
    });

    return list;
  }, [messages]);

  const filteredEvents = useMemo(() => {
    return events.filter((ev) => {
      if (filter === "tools" && ev.kind !== "tool_call") return false;
      if (filter === "messages" && ev.kind !== "user_message" && ev.kind !== "assistant_message") return false;
      if (filter === "think" && ev.kind !== "reasoning") return false;

      if (search.trim()) {
        const q = search.toLowerCase();
        const text = `${ev.title} ${ev.subtitle || ""} ${ev.content || ""}`.toLowerCase();
        return text.includes(q);
      }
      return true;
    });
  }, [events, filter, search]);

  const toggleExpand = (idx: number) => {
    setExpandedIndices((prev) => {
      const next = new Set(prev);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  };

  const expandAll = () => {
    setExpandedIndices(new Set(filteredEvents.map((e) => e.index)));
  };

  const collapseAll = () => {
    setExpandedIndices(new Set());
  };

  const copyFullTrace = () => {
    const traceJson = JSON.stringify(
      {
        totalEvents: events.length,
        conversationCostUsd,
        streamStats,
        events: events.map((e) => ({
          turn: e.turnIndex,
          role: e.role,
          kind: e.kind,
          title: e.title,
          content: e.content,
          duration: e.durationFormatted,
          tokens: e.tokenStats,
          activity: e.activityItem,
        })),
      },
      null,
      2,
    );
    void navigator.clipboard.writeText(traceJson);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div
      className="nowheel nopan nodrag fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4 backdrop-blur-md"
      onClick={onClose}
      onWheel={(e) => e.stopPropagation()}
    >
      <div
        className="nowheel nopan nodrag flex h-[85vh] w-[92vw] max-w-5xl flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[#0b0f17] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
        onWheel={(e) => e.stopPropagation()}
      >
        {/* Modal Header */}
        <div className="flex select-none items-center justify-between border-b border-[var(--border)] bg-[#111827]/80 px-4 py-3">
          <div className="flex items-center gap-3">
            <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-cyan-950/80 border border-cyan-500/30 text-cyan-400 font-mono text-xs">
              <ToolsIcon size={ICON.base} className="text-amber-400" />
            </span>
            <div>
              <h2 className="text-sm font-semibold text-gray-100 flex items-center gap-2 font-mono">
                Agent Trajectory &amp; Event Inspector
                <span className="rounded bg-cyan-500/10 px-1.5 py-0.5 text-[10px] text-cyan-300 font-mono border border-cyan-500/20">
                  {events.length} events
                </span>
              </h2>
              <p className="text-[11px] text-gray-400 font-mono">
                Chronological execution tree, tool invocations, and token metrics (DeepSeek Harness runtime trace)
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={copyFullTrace}
              className="flex items-center gap-1.5 rounded-md border border-[var(--border)] bg-[#162032] px-2.5 py-1 text-[11px] font-mono text-cyan-300 hover:bg-[#1e2d47]"
            >
              {copied ? "✓ Copied JSON" : "Copy Trace JSON"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="flex h-7 w-7 items-center justify-center rounded-md text-gray-400 hover:bg-[var(--border)] hover:text-white"
              title="Close (Esc)"
            >
              <CloseIcon size={ICON.base} />
            </button>
          </div>
        </div>

        {/* Toolbar & Filters */}
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] bg-[#0e1420] px-4 py-2 text-[11px] font-mono">
          <div className="flex items-center gap-1.5">
            {(["all", "tools", "messages"] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                onClick={() => setFilter(mode)}
                className={`rounded px-2.5 py-1 transition ${
                  filter === mode
                    ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/40"
                    : "text-gray-400 hover:bg-[#162032] hover:text-gray-200"
                }`}
              >
                {mode === "all" ? "All Events" : mode === "tools" ? "Tool Invocations" : "Messages"}
              </button>
            ))}
          </div>

          <div className="flex items-center gap-2">
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search trajectory…"
              className="w-48 rounded border border-[var(--border)] bg-[#070a10] px-2.5 py-1 text-[11px] text-gray-200 placeholder-gray-500 focus:border-cyan-500 focus:outline-none"
            />
            <button
              type="button"
              onClick={expandAll}
              className="text-[10px] text-gray-400 hover:text-gray-200 px-1"
            >
              Expand All
            </button>
            <span className="text-gray-600">·</span>
            <button
              type="button"
              onClick={collapseAll}
              className="text-[10px] text-gray-400 hover:text-gray-200 px-1"
            >
              Collapse All
            </button>
          </div>
        </div>

        {/* Event List */}
        <div
          ref={listRef}
          className="nowheel nopan nodrag flex-1 overflow-y-auto p-4 space-y-2.5 font-mono text-[11px]"
          onWheel={(e) => e.stopPropagation()}
        >
          {filteredEvents.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-gray-500">
              <p>No trajectory events found matching filters.</p>
            </div>
          ) : (
            filteredEvents.map((ev) => {
              const isExpanded = expandedIndices.has(ev.index);
              const isUser = ev.role === "user";
              const isTool = ev.kind === "tool_call";

              return (
                <div
                  key={ev.id}
                  className={`rounded-lg border transition-all ${
                    isUser
                      ? "border-blue-900/40 bg-blue-950/20"
                      : isTool
                        ? "border-cyan-900/40 bg-[#0c121e]"
                        : "border-[var(--border)] bg-[#090d15]"
                  }`}
                >
                  {/* Event Header */}
                  <div
                    onClick={() => toggleExpand(ev.index)}
                    className="flex cursor-pointer select-none items-center justify-between gap-3 px-3 py-2 hover:bg-white/5"
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-2.5">
                      <span className="shrink-0 text-[10px] text-gray-500">
                        #{ev.index + 1}
                      </span>
                      <span
                        className={`shrink-0 rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase ${
                          // A failed step is the thing a replay is usually opened to
                          // find, so it is coloured before anything else.
                          ev.activityItem?.state === "error"
                            ? "bg-red-500/20 text-red-300 border border-red-500/30"
                            : isUser
                              ? "bg-blue-500/20 text-blue-300 border border-blue-500/30"
                              : isTool
                                ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/30"
                                : "bg-emerald-500/20 text-emerald-300 border border-emerald-500/30"
                        }`}
                      >
                        {ev.activityItem?.state === "error" ? "failed" : ev.kind.replace("_", " ")}
                      </span>
                      <span className="truncate font-medium text-gray-200">
                        {ev.title}
                      </span>
                    </div>

                    <div className="flex shrink-0 items-center gap-2 text-[10px] text-gray-400">
                      {ev.durationFormatted && (
                        <span className="text-cyan-400"><ClockIcon size={ICON.small} className="inline mr-0.5" /> {ev.durationFormatted}</span>
                      )}
                      {ev.tokenStats?.completionTokens ? (
                        <span className="text-gray-400">
                          {ev.tokenStats.completionTokens} tok
                        </span>
                      ) : null}
                      <span className="text-gray-500">{isExpanded ? <ChevronDownIcon size={ICON.small} /> : <ChevronRightIcon size={ICON.small} />}</span>
                    </div>
                  </div>

                  {/* Expanded Content Payload */}
                  {isExpanded && ev.content && (
                    <div className="border-t border-[var(--border)]/60 bg-[#05080e] p-3 text-[11px] leading-relaxed text-gray-300">
                      <pre
                        className="nowheel nopan nodrag whitespace-pre-wrap break-words font-mono text-[10.5px] max-h-96 overflow-y-auto text-gray-200"
                        onWheel={(e) => e.stopPropagation()}
                      >
                        {maskHost(ev.content)}
                      </pre>
                      {ev.subtitle && (
                        <div className="mt-2 pt-2 border-t border-[var(--border)]/40 text-[10px] text-gray-500">
                          {ev.subtitle}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Modal Footer / Running Cost */}
        <div className="flex items-center justify-between border-t border-[var(--border)] bg-[#0d131f] px-4 py-2 text-[11px] font-mono text-gray-400">
          <div className="flex items-center gap-3">
            <span>Session: {messages.length} messages</span>
            <span>·</span>
            <span className="text-emerald-400 font-semibold">
              Cost: ${conversationCostUsd.toFixed(4)}
            </span>
          </div>
          <div className="text-[10px] text-gray-500">
            Press Esc to close inspector
          </div>
        </div>
      </div>
    </div>
  );
});


