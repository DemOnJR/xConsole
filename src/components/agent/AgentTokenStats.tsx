import type { SessionCacheTotals, TokenStats } from "../../lib/streamStats";
import { cacheBreakdown, formatCacheTooltip } from "../../lib/streamStats";
import { CacheIcon } from "../icons";

/** Compact cache affordance for the composer. Full numbers live in the hover tooltip. */
export function CacheMeter({
  stats,
  sessionCache,
  costUsd,
}: {
  stats: TokenStats | null;
  sessionCache?: SessionCacheTotals | null;
  costUsd?: number;
}) {
  const turn = stats ? cacheBreakdown(stats) : null;
  const sessionRate =
    sessionCache && sessionCache.turns > 0 ? sessionCache.rate : null;
  const rate = turn?.rate ?? sessionRate;
  const pct = rate != null ? Math.round(rate * 100) : null;
  const tooltip = formatCacheTooltip(stats, sessionCache, costUsd);
  if (!tooltip) return null;

  const tone =
    pct == null
      ? "text-[var(--text-faint)]"
      : pct >= 95
        ? "text-emerald-300"
        : pct >= 80
          ? "text-amber-300"
          : "text-red-300";

  return (
    <span
      className={`xc-cache-meter ${tone}`}
      data-tooltip={tooltip}
      data-tooltip-side="top"
      role="img"
      aria-label={tooltip.replace(/\n/g, ", ")}
    >
      <span
        className="xc-cache-rail"
        style={pct != null ? { ["--xc-cache-pct" as string]: `${pct}%` } : undefined}
        aria-hidden
      />
      <CacheIcon size={13} />
    </span>
  );
}
