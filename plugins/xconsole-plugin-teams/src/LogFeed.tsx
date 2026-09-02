import { ConversationIcon, TerminalIcon } from "../../../src/components/icons";
import { logText, type LogLine } from "./log";
import { phaseColor } from "./status";

function clock(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "--:--:--";
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * One agent's log channel: what it is doing, line by line, readable by everyone else.
 *
 * Monospace and one line per action, because this is read the way a tail is read. Every
 * line carries a reply affordance so a correction attaches to the specific action that
 * was wrong -- "that kubectl call hit the wrong context" is useless three hundred lines
 * further down with nothing tying it to the call.
 */
export function LogFeed({
  lines,
  name,
  activeParentId,
  onReply,
  replyCounts,
}: {
  lines: LogLine[];
  name: string;
  activeParentId: string | null;
  onReply: (line: LogLine) => void;
  replyCounts: Map<string, number>;
}) {
  if (lines.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 py-10 text-center">
        <TerminalIcon size={18} className="text-[var(--text-faint)]" />
        <p className="text-[12px] text-[var(--text-dim)]">Nothing logged for {name} yet.</p>
        <p className="max-w-[380px] text-[11px] leading-relaxed text-[var(--text-faint)]">
          Lines appear here as {name} works, and stay readable by the rest of the team
          afterwards. Anything already in flight shows up live.
        </p>
      </div>
    );
  }

  return (
    <div className="px-2 py-2 font-mono text-[11px] leading-[1.6]">
      {lines.map((line) => {
        const replies = replyCounts.get(line.id) ?? 0;
        const active = activeParentId === line.id;
        return (
          <div
            key={line.id}
            className={`group flex items-start gap-2 rounded px-2 py-[3px] ${
              active ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--border)]/30"
            }`}
          >
            <span className="shrink-0 text-[var(--text-faint)]">{clock(line.at)}</span>
            <span
              className="mt-[5px] h-1.5 w-1.5 shrink-0 rounded-full"
              style={{ background: phaseColor(line.status) }}
              title={line.status}
            />
            <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-[var(--text-dim)]">
              {logText(line)}
              {line.live && (
                <span className="ml-2 text-[10px] uppercase tracking-wide text-[var(--text-faint)]">
                  live
                </span>
              )}
            </span>
            {replies > 0 && (
              <span className="shrink-0 rounded-sm bg-[var(--accent-muted)] px-1 text-[10px] text-[var(--text-dim)]">
                {replies}
              </span>
            )}
            <button
              type="button"
              onClick={() => onReply(line)}
              data-tooltip="Reply in thread"
              className={`shrink-0 rounded p-0.5 text-[var(--text-faint)] hover:text-[var(--text)] ${
                replies > 0 || active ? "" : "opacity-0 group-hover:opacity-100"
              }`}
            >
              <ConversationIcon size={12} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
