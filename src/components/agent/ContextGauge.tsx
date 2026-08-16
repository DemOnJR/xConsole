import { useState, type PointerEvent } from "react";
import type { ContextUsage } from "../../lib/streamStats";

/** Circular context-window gauge: donut of % used, hover popover with
 *  used/max + segment breakdown. Clicking opens the /ctx breakdown (handled by the
 *  parent via onClick). */
export function ContextGauge({
  usage,
  onClick,
  onPointerDown,
}: {
  usage: ContextUsage | null;
  onClick?: () => void;
  onPointerDown?: (e: PointerEvent<HTMLButtonElement>) => void;
}) {
  const [hover, setHover] = useState(false);
  const pct = usage ? Math.min(100, Math.round(usage.percent)) : 0;
  const r = 8;
  const c = 2 * Math.PI * r;
  const dash = (pct / 100) * c;
  const hot = pct >= 90;

  return (
    <div
      className="relative"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      <button
        type="button"
        onClick={onClick}
        onPointerDown={onPointerDown}
        onMouseDown={(e) => e.stopPropagation()}
        aria-label={usage ? `Context window ${pct}% used` : "Context usage"}
        className="flex items-center gap-1 rounded px-1 py-0.5 text-[10px] text-[var(--text-faint)] hover:text-[var(--text)]"
      >
        <svg width="18" height="18" viewBox="0 0 20 20" className="-rotate-90">
          <circle cx="10" cy="10" r={r} fill="none" stroke="var(--border)" strokeWidth="3" />
          <circle
            cx="10"
            cy="10"
            r={r}
            fill="none"
            stroke={hot ? "var(--danger, #f87171)" : "var(--accent, #3b82f6)"}
            strokeWidth="3"
            strokeDasharray={`${dash} ${c - dash}`}
            strokeLinecap="round"
          />
        </svg>
        <span className="xc-gauge-pct tabular-nums">{usage ? `${pct}%` : "—"}</span>
      </button>

      {hover && usage && (
        <div className="absolute bottom-full right-0 z-30 mb-1 w-56 rounded-md border border-[var(--border)] bg-[var(--surface)] p-2.5 shadow-2xl">
          <div className="mb-1 flex items-baseline justify-between">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
              Context
            </span>
            <span className="text-[10px] tabular-nums text-[var(--text)]">
              {usage.percent}% · {usage.total_tokens.toLocaleString()}/
              {usage.context_limit.toLocaleString()}
            </span>
          </div>
          <div className="mb-2 h-1 overflow-hidden rounded bg-[var(--border)]">
            <div
              className={`h-full ${hot ? "bg-red-500" : "bg-blue-500"}`}
              style={{ width: `${pct}%` }}
            />
          </div>
          {usage.segments.map((s) => (
            <div key={s.key} className="flex justify-between gap-2 text-[10px] text-[var(--text-faint)]">
              <span className="truncate">{s.label}</span>
              <span className="tabular-nums">{s.tokens.toLocaleString()}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
