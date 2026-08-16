import { useMemo, useState } from "react";
import type { AgentActivityItem, AgentChatMessage, TurnSegment } from "../../stores/agentStore";
import { isTodoItem } from "./AgentActivity";

export interface ChecklistItem {
  status: "done" | "active" | "pending";
  text: string;
  raw: string;
}

export function parseChecklist(rawText: string): ChecklistItem[] {
  const lines = rawText.split("\n").filter((l) => l.trim().length > 0);
  return lines.map((line) => {
    const trimmed = line.trim();
    if (trimmed.startsWith("[x]") || trimmed.startsWith("- [x]") || trimmed.startsWith("* [x]")) {
      return {
        status: "done",
        text: trimmed.replace(/^[-*]?\s*\[x\]\s*/i, ""),
        raw: trimmed,
      };
    }
    if (
      trimmed.startsWith("[>]") ||
      trimmed.startsWith("- [>]") ||
      trimmed.startsWith("* [>]") ||
      trimmed.startsWith("[*]")
    ) {
      return {
        status: "active",
        text: trimmed.replace(/^[-*]?\s*\[[>*]\]\s*/i, ""),
        raw: trimmed,
      };
    }
    return {
      status: "pending",
      text: trimmed.replace(/^[-*]?\s*\[\s*\]\s*/i, ""),
      raw: trimmed,
    };
  });
}

/** Find the most recent todo_write item across live turn and messages history. */
export function findLatestChecklist(
  messages: AgentChatMessage[],
  streamingSegments: TurnSegment[] = [],
  liveActivity: AgentActivityItem[] = [],
): string | null {
  // Check live activity first
  for (let i = liveActivity.length - 1; i >= 0; i--) {
    const item = liveActivity[i];
    if (isTodoItem(item) && (item.output || item.detail)) {
      return (item.output || item.detail)!.trim();
    }
  }

  // Check streaming segments
  for (let i = streamingSegments.length - 1; i >= 0; i--) {
    const seg = streamingSegments[i];
    if (seg.type === "activity") {
      for (let j = seg.items.length - 1; j >= 0; j--) {
        const item = seg.items[j];
        if (isTodoItem(item) && (item.output || item.detail)) {
          return (item.output || item.detail)!.trim();
        }
      }
    }
  }

  // Check messages history in reverse
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i];
    if (msg.activity) {
      for (let j = msg.activity.length - 1; j >= 0; j--) {
        const item = msg.activity[j];
        if (isTodoItem(item) && (item.output || item.detail)) {
          return (item.output || item.detail)!.trim();
        }
      }
    }
    if (msg.segments) {
      for (let j = msg.segments.length - 1; j >= 0; j--) {
        const seg = msg.segments[j];
        if (seg.type === "activity") {
          for (let k = seg.items.length - 1; k >= 0; k--) {
            const item = seg.items[k];
            if (isTodoItem(item) && (item.output || item.detail)) {
              return (item.output || item.detail)!.trim();
            }
          }
        }
      }
    }
  }

  return null;
}

/** Elegant inline card in chat when a checklist is 100% complete with execution duration. */
export function CompletedChecklistCard({
  rawChecklist,
  duration,
}: {
  rawChecklist: string;
  duration?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const items = useMemo(() => parseChecklist(rawChecklist), [rawChecklist]);
  if (items.length === 0) return null;

  const doneCount = items.filter((i) => i.status === "done").length;
  const totalCount = items.length;

  return (
    <div className="my-2 flex w-full flex-col overflow-hidden rounded-lg border border-emerald-500/30 bg-emerald-950/20 shadow-sm backdrop-blur-sm">
      <div
        onClick={() => setExpanded((v) => !v)}
        className="flex cursor-pointer select-none items-center justify-between gap-2 px-3 py-2 text-[11px] font-mono hover:bg-emerald-950/30"
      >
        <div className="flex items-center gap-2 min-w-0">
          <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-emerald-500/20 text-emerald-400 font-bold">
            ✓
          </span>
          <span className="font-semibold text-emerald-300">
            Completed all {doneCount}/{totalCount} tasks
          </span>
          {duration && (
            <span className="text-[10px] text-emerald-400/80 font-normal">
              · {duration}
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setExpanded((v) => !v);
          }}
          className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-emerald-400/80 hover:bg-emerald-500/20 hover:text-emerald-200"
        >
          <span>{expanded ? "Hide details ▾" : "Show details ▸"}</span>
        </button>
      </div>

      {expanded && (
        <div className="border-t border-emerald-500/20 bg-black/40 px-3 py-2">
          <ul className="flex flex-col gap-1 font-mono text-[11px]">
            {items.map((item, idx) => (
              <li key={idx} className="flex items-start gap-2 text-emerald-300/80">
                <span className="shrink-0 text-emerald-400 font-bold">✓</span>
                <span className="flex-1 line-through opacity-80 break-words">{item.text}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

export function StickyChecklist({
  rawChecklist,
  streaming = false,
  position = "bottom",
}: {
  rawChecklist: string | null;
  streaming?: boolean;
  position?: "top" | "bottom";
}) {
  const [collapsed, setCollapsed] = useState(true);
  const [dismissed, setDismissed] = useState(false);

  const items = useMemo(() => {
    if (!rawChecklist) return [];
    return parseChecklist(rawChecklist);
  }, [rawChecklist]);

  if (!rawChecklist || items.length === 0 || dismissed) return null;

  const doneCount = items.filter((i) => i.status === "done").length;
  const totalCount = items.length;
  const activeItem =
    items.find((i) => i.status === "active") || items.find((i) => i.status === "pending");
  const allDone = doneCount === totalCount && totalCount > 0;
  const isBottom = position === "bottom";

  // When all tasks are completed and turn is idle, the checklist is rendered in chat — auto-hide sticky bar!
  if (allDone && !streaming) {
    return null;
  }

  const renderExpandedList = () => (
    <div
      className={`max-h-48 overflow-y-auto bg-black/50 px-3 py-2 ${
        isBottom ? "border-b border-[var(--border)]/60" : "border-t border-[var(--border)]/60"
      }`}
    >
      <ul className="flex flex-col gap-1 font-mono text-[11px]">
        {items.map((item, idx) => {
          const isDone = item.status === "done";
          const isActive = item.status === "active";
          return (
            <li
              key={idx}
              className={`flex items-start gap-2 rounded px-1.5 py-0.5 ${
                isActive
                  ? "border-l-2 border-cyan-400 bg-cyan-950/40 font-medium text-cyan-200"
                  : isDone
                    ? "line-through opacity-70 text-gray-500"
                    : "text-gray-300 hover:text-gray-100"
              }`}
            >
              <span
                className={`shrink-0 font-bold ${
                  isActive
                    ? "text-cyan-400"
                    : isDone
                      ? "text-emerald-500"
                      : "text-gray-500"
                }`}
              >
                {isDone ? "✓" : isActive ? "▶" : "○"}
              </span>
              <span className="flex-1 break-words">{item.text}</span>
            </li>
          );
        })}
      </ul>
    </div>
  );

  return (
    <div
      className={`z-30 flex w-full flex-col bg-[#0c1017]/95 shadow-md backdrop-blur-md transition-all ${
        isBottom
          ? "sticky bottom-0 border-t border-[var(--border)]"
          : "sticky top-0 border-b border-[var(--border)]"
      }`}
    >
      {/* If positioned at bottom, expanded list renders ABOVE the summary header */}
      {isBottom && !collapsed && renderExpandedList()}

      {/* Header bar (always visible) */}
      <div
        onClick={() => setCollapsed((v) => !v)}
        className="flex cursor-pointer select-none items-center justify-between gap-2 px-3 py-1.5 text-[11px] font-mono hover:bg-[var(--surface-hover)]"
      >
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {/* Status Badge */}
          <span
            className={`flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold ${
              allDone
                ? "border border-emerald-500/40 bg-emerald-950/60 text-emerald-300"
                : "border border-cyan-500/40 bg-cyan-950/60 text-cyan-300"
            }`}
          >
            {streaming && !allDone && (
              <span className="relative flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-cyan-400 opacity-75"></span>
                <span className="relative inline-flex h-2 w-2 rounded-full bg-cyan-500"></span>
              </span>
            )}
            Checklist {doneCount}/{totalCount}
          </span>

          {/* Collapsed active item preview */}
          {collapsed && activeItem && (
            <div className="flex min-w-0 flex-1 items-center gap-1.5 truncate text-[11px]">
              <span
                className={`font-bold ${
                  activeItem.status === "active" ? "text-cyan-400" : "text-gray-400"
                }`}
              >
                {activeItem.status === "active" ? "▶" : "○"}
              </span>
              <span
                className={`truncate ${
                  activeItem.status === "active"
                    ? "font-medium text-cyan-100"
                    : "text-gray-300"
                }`}
              >
                {activeItem.text}
              </span>
            </div>
          )}
        </div>

        {/* Action buttons */}
        <div className="flex items-center gap-1.5">
          {/* Toggle Expand / Collapse button */}
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setCollapsed((v) => !v);
            }}
            className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-white"
          >
            {collapsed ? (
              <>
                <span>{totalCount - doneCount} left</span>
                <span>{isBottom ? "▴" : "▾"}</span>
              </>
            ) : (
              <>
                <span>Collapse</span>
                <span>{isBottom ? "▾" : "▴"}</span>
              </>
            )}
          </button>

          {/* Dismiss button */}
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setDismissed(true);
            }}
            className="flex shrink-0 items-center rounded p-1 text-gray-500 hover:bg-[var(--border)] hover:text-gray-300"
            title="Dismiss checklist bar"
          >
            ✕
          </button>
        </div>
      </div>

      {/* If positioned at top, expanded list renders BELOW the summary header */}
      {!isBottom && !collapsed && renderExpandedList()}
    </div>
  );
}
