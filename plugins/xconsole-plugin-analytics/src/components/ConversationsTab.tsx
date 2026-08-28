import React, { useState, useMemo, useDeferredValue } from "react";
import type { AgentAnalytics, ConversationStat, ToolCount } from "../types";
import {
  ChatIcon,
  SearchIcon,
  ToolIcon,
  ChevronRightIcon,
  ActivityIcon,
} from "../icons";

interface ConversationsTabProps {
  data: AgentAnalytics;
}

export const ConversationsTab = React.memo(function ConversationsTab({ data }: ConversationsTabProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearch = useDeferredValue(searchQuery);
  const [selectedId, setSelectedId] = useState<string | null>(
    data.conversations[0]?.id || null,
  );

  const filteredConversations = useMemo(() => {
    const q = deferredSearch.toLowerCase().trim();
    if (!q) return data.conversations;
    return data.conversations.filter(
      (c: ConversationStat) =>
        (c.title || "").toLowerCase().includes(q) ||
        c.id.toLowerCase().includes(q),
    );
  }, [data.conversations, deferredSearch]);

  const selectedConv = useMemo(
    () => data.conversations.find((c: ConversationStat) => c.id === selectedId) || null,
    [data.conversations, selectedId],
  );

  const totalTurns = useMemo(
    () => data.conversations.reduce((acc: number, c: ConversationStat) => acc + c.user_turns, 0),
    [data.conversations],
  );
  const totalTools = useMemo(
    () => data.conversations.reduce((acc: number, c: ConversationStat) => acc + c.tool_calls, 0),
    [data.conversations],
  );

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Summary Cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="flex flex-col justify-between rounded-xl border border-rose-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-rose-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Saved Sessions</span>
            <ChatIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-rose-300">
            {data.conversations.length}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            conversations with tool execution history
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-cyan-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-cyan-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Total User Turns</span>
            <ActivityIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-cyan-300">
            {totalTurns}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            interactive dialogue prompt rounds
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-purple-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-purple-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Tools Executed</span>
            <ToolIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-purple-300">
            {totalTools}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            avg {(totalTools / Math.max(1, totalTurns)).toFixed(1)} tools per user turn
          </div>
        </div>
      </div>

      {/* 2. Split Layout: Conversation List & Detail Inspector */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-12 min-h-[380px]">
        {/* Left Column: List */}
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4 lg:col-span-6 flex flex-col">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-3 mb-3">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              Sessions ({filteredConversations.length})
            </h3>
            <div className="relative flex items-center">
              <div className="pointer-events-none absolute left-2.5 text-[var(--text-faint,#71717a)]">
                <SearchIcon size={13} />
              </div>
              <input
                type="text"
                placeholder="Search session..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="h-7 w-40 rounded-md border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] pl-7 pr-2 text-xs text-[var(--text,#e4e4e7)] placeholder-[var(--text-faint,#71717a)] focus:border-rose-500 focus:outline-none"
              />
            </div>
          </div>

          {filteredConversations.length === 0 ? (
            <div className="my-auto flex flex-col items-center justify-center py-12 text-center text-xs text-[var(--text-faint,#71717a)]">
              <ChatIcon size={24} className="mb-2 opacity-40" />
              No conversations recorded yet.
            </div>
          ) : (
            <div className="flex flex-col gap-2 overflow-y-auto max-h-[420px] pr-1">
              {filteredConversations.map((c: ConversationStat) => {
                const isSelected = c.id === selectedId;
                return (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => setSelectedId(c.id)}
                    className={`flex items-center justify-between rounded-lg p-3 text-left transition border ${
                      isSelected
                        ? "border-rose-500/40 bg-rose-500/10 text-white shadow-sm"
                        : "border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)]/50 text-[var(--text-dim,#a1a1aa)] hover:border-[var(--border,#3f3f46)] hover:bg-[var(--surface-2,#18181b)]"
                    }`}
                  >
                    <div className="min-w-0 flex-1 pr-3">
                      <div className="font-medium text-xs text-[var(--text,#fafafa)] truncate">
                        {c.title || "Untitled Conversation"}
                      </div>
                      <div className="mt-1 flex items-center gap-2 font-mono text-[11px] text-[var(--text-faint,#71717a)]">
                        <span>{c.user_turns} turns</span>
                        <span>&bull;</span>
                        <span className="text-purple-400">{c.tool_calls} tools</span>
                        {c.updated_at && (
                          <>
                            <span>&bull;</span>
                            <span className="truncate">{c.updated_at.slice(0, 16).replace("T", " ")}</span>
                          </>
                        )}
                      </div>
                    </div>
                    <ChevronRightIcon
                      size={14}
                      className={isSelected ? "text-rose-400" : "text-zinc-600"}
                    />
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {/* Right Column: Inspector */}
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4 lg:col-span-6 flex flex-col justify-between">
          <div>
            <div className="border-b border-[var(--border,#27272a)] pb-3 mb-3">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
                Session Inspector
              </h3>
            </div>

            {!selectedConv ? (
              <div className="my-auto flex flex-col items-center justify-center py-16 text-center text-xs text-[var(--text-faint,#71717a)]">
                Select a conversation on the left to inspect tool activity.
              </div>
            ) : (
              <div className="flex flex-col gap-4">
                <div>
                  <h4 className="text-sm font-semibold text-white">
                    {selectedConv.title || "Untitled Conversation"}
                  </h4>
                  <div className="mt-1 font-mono text-xs text-[var(--text-faint,#71717a)] break-all">
                    ID: {selectedConv.id}
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-2 text-xs">
                  <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-2.5 border border-[var(--border,#27272a)]">
                    <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">
                      User Prompt Turns
                    </div>
                    <div className="font-mono text-base font-bold text-cyan-300">
                      {selectedConv.user_turns}
                    </div>
                  </div>
                  <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-2.5 border border-[var(--border,#27272a)]">
                    <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">
                      Tool Invocations
                    </div>
                    <div className="font-mono text-base font-bold text-purple-300">
                      {selectedConv.tool_calls}
                    </div>
                  </div>
                </div>

                <div>
                  <h5 className="text-xs font-medium text-[var(--text-dim,#a1a1aa)] mb-2">
                    Tools Used in this Session ({selectedConv.tools.length}):
                  </h5>

                  {selectedConv.tools.length === 0 ? (
                    <p className="text-xs text-[var(--text-faint,#71717a)] italic">
                      No tool calls recorded in this session.
                    </p>
                  ) : (
                    <div className="flex flex-col gap-2 max-h-[220px] overflow-y-auto pr-1">
                      {selectedConv.tools.map((t: ToolCount) => (
                        <div
                          key={t.name}
                          className="flex items-center justify-between rounded-md bg-[var(--surface-2,#18181b)] px-3 py-2 text-xs font-mono border border-[var(--border,#27272a)]"
                        >
                          <span className="text-[var(--text,#fafafa)]">{t.name}</span>
                          <span className="rounded bg-purple-500/10 border border-purple-500/20 px-2 py-0.5 text-purple-300 font-semibold">
                            &times;{t.count}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          {selectedConv?.updated_at && (
            <div className="mt-4 pt-3 border-t border-[var(--border,#27272a)] text-[11px] font-mono text-[var(--text-faint,#71717a)] flex items-center justify-between">
              <span>Last updated:</span>
              <span>{selectedConv.updated_at.replace("T", " ")}</span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
});
