import type { SessionCacheTotals, TokenStats } from "../../../src/lib/streamStats";
import { cacheBreakdown, formatCacheTooltip } from "../../../src/lib/streamStats";
import { CacheIcon } from "../../../src/components/icons";

/** Compact cache affordance for the composer. Full numbers live in the hover tooltip. */
export function CacheMeter({
  stats,
  sessionCache,
  costUsd,
  onClick,
}: {
  stats: TokenStats | null;
  sessionCache?: SessionCacheTotals | null;
  costUsd?: number;
  onClick?: () => void;
}) {
  const turn = stats ? cacheBreakdown(stats) : null;
  const sessionRate =
    sessionCache && sessionCache.turns > 0 ? sessionCache.rate : null;
  const rate = turn?.rate ?? sessionRate;
  const pct = rate != null ? Math.round(rate * 100) : null;
  const tooltip =
    formatCacheTooltip(stats, sessionCache, costUsd) ||
    "Prompt cache · 0 turns\nLive token hit/miss stats";

  const tone =
    pct == null
      ? "text-[var(--text-faint)]"
      : pct >= 95
        ? "text-emerald-300"
        : pct >= 80
          ? "text-amber-300"
          : "text-red-300";

  return (
    <button
      type="button"
      className={`xc-cache-meter flex items-center justify-center ${tone} bg-transparent border-0 p-0 outline-none`}
      data-tooltip={tooltip}
      data-tooltip-side="top"
      aria-label={tooltip.replace(/\n/g, ", ")}
      onClick={onClick}
      onPointerDown={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <span
        className="xc-cache-rail"
        style={pct != null ? { ["--xc-cache-pct" as string]: `${pct}%` } : undefined}
        aria-hidden
      />
      <CacheIcon size={13} />
    </button>
  );
}
