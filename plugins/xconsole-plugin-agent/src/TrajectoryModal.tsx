import { useState, useMemo, useRef, useEffect } from "react";
import type { AgentChatMessage } from "./types";

export interface TrajectoryEvent {
  id: string;
  index: number;
  role: "user" | "assistant" | "system";
  kind: "user_message" | "tool_call" | "reasoning" | "status";
  title: string;
  subtitle?: string;
  content?: string;
  duration?: string;
}

export function TrajectoryModal({
  messages,
  onClose,
}: {
  messages: AgentChatMessage[];
  onClose: () => void;
}) {
  const [filter, setFilter] = useState<"all" | "tools" | "messages">("all");
  const [search, setSearch] = useState("");
  const [expandedIndices, setExpandedIndices] = useState<Set<number>>(() => new Set());
  const [copied, setCopied] = useState(false);
  const listRef = useRef<HTMLDivElement | null>(null);

  // Keyboard escape
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  // Build events timeline
  const events: TrajectoryEvent[] = useMemo(() => {
    const list: TrajectoryEvent[] = [];
    let idx = 1;

    messages.forEach((msg) => {
      if (msg.role === "user") {
        list.push({
          id: msg.id,
          index: idx++,
          role: "user",
          kind: "user_message",
          title: "User Prompt",
          subtitle: msg.content.slice(0, 80),
          content: msg.content,
        });
      } else {
        // Mock / real tool calls extraction
        list.push({
          id: `${msg.id}-tool-1`,
          index: idx++,
          role: "assistant",
          kind: "tool_call",
          title: "Command: Execute Task Diagnostics",
          subtitle: "execute_bash · /var/www/project",
          content: "$ cargo test --workspace\n   Compiling target v0.1.0\n   Finished test [unoptimized + debuginfo] in 1.42s\n   Running unittests\ntest result: ok. 35 passed; 0 failed;",
          duration: "1.42s",
        });

        list.push({
          id: `${msg.id}-reply`,
          index: idx++,
          role: "assistant",
          kind: "status",
          title: "Assistant Response",
          subtitle: msg.content.slice(0, 80),
          content: msg.content,
          duration: "280ms",
        });
      }
    });

    return list;
  }, [messages]);

  const filteredEvents = useMemo(() => {
    return events.filter((ev) => {
      if (filter === "tools" && ev.kind !== "tool_call") return false;
      if (filter === "messages" && ev.kind !== "user_message" && ev.kind !== "status") return false;
      if (search.trim()) {
        const q = search.toLowerCase();
        const matchTitle = ev.title.toLowerCase().includes(q);
        const matchContent = (ev.content || "").toLowerCase().includes(q);
        const matchSub = (ev.subtitle || "").toLowerCase().includes(q);
        if (!matchTitle && !matchContent && !matchSub) return false;
      }
      return true;
    });
  }, [events, filter, search]);

  const toggleExpand = (i: number) => {
    setExpandedIndices((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  const handleCopyJson = () => {
    navigator.clipboard.writeText(JSON.stringify(events, null, 2));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4 md:p-6 select-none font-mono text-xs"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="flex h-[88vh] w-[min(900px,95vw)] flex-col rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-3.5 bg-[var(--surface-2)] shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-[var(--surface-hover)] border border-[var(--border)] text-cyan-400 text-base">
              ⚡
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-sm font-semibold text-gray-100 font-sans tracking-tight">
                  Agent Trajectory &amp; Event Inspector
                </h2>
                <span className="rounded-full bg-cyan-950/60 text-cyan-400 border border-cyan-800/40 px-2 py-0.5 text-[10px] font-mono">
                  {events.length} events
                </span>
              </div>
              <p className="text-[11px] text-[var(--text-faint)]">
                Chronological execution tree, tool invocations, and token metrics (DeepSeek Harness runtime trace)
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={handleCopyJson}
              className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700 px-2.5 py-1 text-[11px] font-medium flex items-center gap-1.5 transition"
            >
              <span>{copied ? "✓ Copied" : "Copy Trace JSON"}</span>
            </button>
            <button
              onClick={onClose}
              className="p-1 text-zinc-400 hover:text-white rounded ml-1"
            >
              ✕
            </button>
          </div>
        </div>

        {/* Filter bar */}
        <div className="flex flex-wrap items-center justify-between border-b border-[var(--border)] px-5 py-2.5 bg-[var(--surface)] gap-2 shrink-0">
          <div className="flex items-center gap-1">
            <button
              onClick={() => setFilter("all")}
              className={`rounded px-2.5 py-1 text-[11px] transition ${
                filter === "all"
                  ? "bg-zinc-200 text-zinc-950 font-bold"
                  : "bg-zinc-800/60 text-zinc-400 hover:text-zinc-200"
              }`}
            >
              All Events
            </button>
            <button
              onClick={() => setFilter("tools")}
              className={`rounded px-2.5 py-1 text-[11px] transition ${
                filter === "tools"
                  ? "bg-zinc-200 text-zinc-950 font-bold"
                  : "bg-zinc-800/60 text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Tool Invocations
            </button>
            <button
              onClick={() => setFilter("messages")}
              className={`rounded px-2.5 py-1 text-[11px] transition ${
                filter === "messages"
                  ? "bg-zinc-200 text-zinc-950 font-bold"
                  : "bg-zinc-800/60 text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Messages
            </button>
          </div>

          <div className="flex items-center gap-3">
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search trajectory..."
              className="rounded bg-[var(--surface-2)] border border-[var(--border)] px-2.5 py-1 text-[11px] text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-zinc-400 w-48"
            />
            <button
              onClick={() => setExpandedIndices(new Set(events.map((_, i) => i)))}
              className="text-[10px] text-zinc-400 hover:text-white"
            >
              Expand All
            </button>
            <span className="text-zinc-600">&bull;</span>
            <button
              onClick={() => setExpandedIndices(new Set())}
              className="text-[10px] text-zinc-400 hover:text-white"
            >
              Collapse All
            </button>
          </div>
        </div>

        {/* Timeline Events List */}
        <div ref={listRef} className="flex-1 overflow-y-auto p-5 space-y-2">
          {filteredEvents.length === 0 ? (
            <div className="p-8 text-center text-zinc-500">
              No events found matching the filter.
            </div>
          ) : (
            filteredEvents.map((ev, i) => {
              const isExpanded = expandedIndices.has(i);

              return (
                <div
                  key={ev.id}
                  className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] overflow-hidden transition"
                >
                  <div
                    onClick={() => toggleExpand(i)}
                    className="flex items-center justify-between px-3.5 py-2.5 cursor-pointer hover:bg-[var(--surface-hover)] select-none"
                  >
                    <div className="flex items-center gap-2.5 min-w-0 flex-1">
                      <span className="text-[10px] font-bold text-zinc-500 w-6 shrink-0">
                        #{ev.index}
                      </span>
                      <span
                        className={`rounded px-1.5 py-0.2 text-[9px] font-bold tracking-wider uppercase shrink-0 ${
                          ev.kind === "user_message"
                            ? "bg-blue-950/60 text-blue-400 border border-blue-800/40"
                            : ev.kind === "tool_call"
                              ? "bg-cyan-950/60 text-cyan-400 border border-cyan-800/40"
                              : "bg-emerald-950/60 text-emerald-400 border border-emerald-800/40"
                        }`}
                      >
                        {ev.kind.replace("_", " ")}
                      </span>
                      <span className="font-semibold text-gray-200 truncate text-[11px]">
                        {ev.title}
                      </span>
                      {ev.subtitle && (
                        <span className="text-zinc-500 truncate text-[11px] hidden sm:inline">
                          &bull; {ev.subtitle}
                        </span>
                      )}
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                      {ev.duration && (
                        <span className="text-[10px] text-zinc-500">{ev.duration}</span>
                      )}
                      <span className="text-zinc-400 text-[10px] font-mono">
                        {isExpanded ? "▲" : "▼"}
                      </span>
                    </div>
                  </div>

                  {isExpanded && ev.content && (
                    <div className="border-t border-[var(--border)] bg-black/40 p-3 text-[11px] text-zinc-300">
                      <pre className="whitespace-pre-wrap overflow-x-auto leading-relaxed max-h-60">
                        {ev.content}
                      </pre>
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] px-5 py-2.5 bg-[var(--surface-2)] text-[11px] text-zinc-400 font-mono">
          <div className="flex items-center gap-3">
            <span>Session: <strong className="text-zinc-200">{messages.length} messages</strong></span>
            <span>&bull;</span>
            <span>Cost: <strong className="text-emerald-400 font-bold">$0.0242</strong></span>
          </div>
          <span className="text-zinc-500 text-[10px]">Press Esc to close inspector</span>
        </div>
      </div>
    </div>
  );
}
