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

export function StickyChecklist({
  rawChecklist,
  streaming = false,
}: {
  rawChecklist: string | null;
  streaming?: boolean;
}) {
  const [collapsed, setCollapsed] = useState(true);

  const items = useMemo(() => {
    if (!rawChecklist) return [];
    return parseChecklist(rawChecklist);
  }, [rawChecklist]);

  if (!rawChecklist || items.length === 0) return null;

  const doneCount = items.filter((i) => i.status === "done").length;
  const totalCount = items.length;
  const activeItem =
    items.find((i) => i.status === "active") || items.find((i) => i.status === "pending");
  const allDone = doneCount === totalCount;

  return (
    <div className="sticky top-0 z-30 flex w-full flex-col border-b border-[var(--border)] bg-[#0c1017]/95 shadow-md backdrop-blur-md transition-all">
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
              <span>▾</span>
            </>
          ) : (
            <>
              <span>Collapse</span>
              <span>▴</span>
            </>
          )}
        </button>
      </div>

      {/* Expanded full list */}
      {!collapsed && (
        <div className="max-h-48 overflow-y-auto border-t border-[var(--border)]/60 bg-black/40 px-3 py-2">
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
      )}
    </div>
  );
}
