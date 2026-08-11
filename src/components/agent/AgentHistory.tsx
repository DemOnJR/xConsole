import { useState } from "react";
import type { AgentConversationMeta } from "../../lib/tauri";
import type { AgentActivityItem } from "../../stores/agentStore";

function formatWhen(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso.includes("T") ? iso : `${iso.replace(" ", "T")}Z`);
  if (Number.isNaN(d.getTime())) return iso.slice(0, 10);
  const now = new Date();
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  if (sameDay) {
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}

function shortTitle(title: string, max = 22): string {
  const t = title.trim() || "Untitled";
  if (t.length <= max) return t;
  return `${t.slice(0, max - 1)}…`;
}

/** Always-visible recent conversation chips for fast multi-session switching. */
export function AgentSessionTabs({
  conversations,
  activeId,
  onSelect,
  onNew,
  onRename,
  disabled,
}: {
  conversations: AgentConversationMeta[];
  activeId: string;
  onSelect: (id: string) => void;
  onNew: () => void;
  onRename?: (id: string, title: string) => void;
  disabled?: boolean;
}) {
  // Active first, then most recently updated.
  const tabs = [...conversations]
    .sort((a, b) => {
      if (a.id === activeId) return -1;
      if (b.id === activeId) return 1;
      return (b.updated_at || "").localeCompare(a.updated_at || "");
    })
    .slice(0, 6);

  if (tabs.length === 0) return null;

  return (
    <div className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-[var(--border)]/80 bg-[var(--bg)]/80 px-2 py-1">
      {tabs.map((c) => {
        const active = c.id === activeId;
        return (
          <button
            key={c.id}
            type="button"
            disabled={disabled && !active}
            onClick={() => onSelect(c.id)}
            onDoubleClick={() => {
              if (!onRename) return;
              const next = window.prompt("Rename conversation", c.title);
              if (next != null && next.trim()) onRename(c.id, next.trim());
            }}
            className={`max-w-[140px] shrink-0 truncate rounded-md px-2 py-0.5 text-[10px] transition ${
              active
                ? "bg-blue-600/30 text-blue-100 ring-1 ring-inset ring-blue-500/40"
                : "bg-[var(--surface)] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            } disabled:opacity-40`}
            data-tooltip={`${c.title} — double-click to rename`}
          >
            {shortTitle(c.title)}
          </button>
        );
      })}
      <button
        type="button"
        disabled={disabled}
        onClick={onNew}
        className="shrink-0 rounded-md px-1.5 py-0.5 text-[10px] text-gray-500 hover:bg-[var(--border)] hover:text-gray-300 disabled:opacity-40"
        data-tooltip="New conversation"
      >
        +
      </button>
    </div>
  );
}

/** Thin live line under session tabs while the agent is working. */
export function AgentLiveStatus({
  streaming,
  activity,
  planMode,
}: {
  streaming: boolean;
  activity: AgentActivityItem[];
  planMode?: boolean;
}) {
  if (!streaming && !planMode) return null;
  const running = activity.filter((a) => a.state === "running");
  const label = !streaming
    ? planMode
      ? "Plan mode — read-only until you approve"
      : null
    : running.length > 1
      ? `Running ${running.length} tools in parallel…`
      : running.length === 1
        ? running[0].label || "Working…"
        : "Thinking…";
  if (!label) return null;
  return (
    <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border)]/60 bg-[var(--surface)]/40 px-2.5 py-1">
      {streaming ? (
        <span
          className="inline-block h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-cyan-400"
          aria-hidden
        />
      ) : (
        <span className="inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400/80" />
      )}
      <span className="min-w-0 flex-1 truncate text-[10px] text-gray-400">{label}</span>
      {running.length > 0 ? (
        <span className="shrink-0 font-mono text-[9px] text-cyan-500/80">
          {running.length} active
        </span>
      ) : null}
    </div>
  );
}

export function AgentHistory({
  open,
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
  onRename,
  onClose,
}: {
  open: boolean;
  conversations: AgentConversationMeta[];
  activeId: string;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename?: (id: string, title: string) => void;
  onClose: () => void;
}) {
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const list = !q
    ? conversations
    : conversations.filter(
        (c) =>
          c.title.toLowerCase().includes(q) ||
          (c.summary ?? "").toLowerCase().includes(q),
      );

  if (!open) return null;

  return (
    <div className="border-b border-[var(--border)] bg-[var(--bg)] px-2 py-2">
      <div className="mb-2 flex items-center gap-2 px-1">
        <span className="text-[11px] font-medium text-gray-400">History</span>
        <button
          type="button"
          onClick={onNew}
          className="ml-auto rounded border border-[var(--border)] px-2 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"
        >
          + New
        </button>
        <button
          type="button"
          onClick={onClose}
          className="text-[10px] text-gray-500 hover:text-gray-300"
        >
          hide
        </button>
      </div>
      {conversations.length > 4 ? (
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter chats…"
          className="mb-1.5 w-full rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-1 text-[11px] text-gray-200 outline-none focus:border-blue-600"
        />
      ) : null}
      <div className="max-h-40 space-y-0.5 overflow-y-auto">
        {list.length === 0 && (
          <p className="px-1 py-2 text-[10px] text-gray-600">
            {conversations.length === 0 ? "No saved chats yet." : "No matches."}
          </p>
        )}
        {list.map((c) => {
          const active = c.id === activeId;
          return (
            <div
              key={c.id}
              className={`group flex items-start gap-1 rounded px-1.5 py-1 ${
                active ? "bg-blue-600/20" : "hover:bg-[var(--border)]/60"
              }`}
            >
              <button
                type="button"
                className="min-w-0 flex-1 text-left"
                onClick={() => onSelect(c.id)}
                onDoubleClick={() => {
                  if (!onRename) return;
                  const next = window.prompt("Rename conversation", c.title);
                  if (next != null && next.trim()) onRename(c.id, next.trim());
                }}
              >
                <div className="truncate text-[11px] text-gray-200" title="Double-click to rename">
                  {c.title}
                </div>
                {c.summary && (
                  <div className="mt-0.5 line-clamp-2 text-[10px] leading-snug text-gray-500">
                    {c.summary.replace(/^-\s*/gm, "").slice(0, 120)}
                  </div>
                )}
                <div className="mt-0.5 text-[9px] text-gray-600">{formatWhen(c.updated_at)}</div>
              </button>
              <button
                type="button"
                data-tooltip="Delete"
                onClick={() => onDelete(c.id)}
                className="shrink-0 px-1 text-[10px] text-gray-600 opacity-0 hover:text-red-400 group-hover:opacity-100"
              >
                ✕
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
