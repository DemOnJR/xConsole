import type { TokenStats, TurnTelemetry } from "../../lib/streamStats";
import { cacheBreakdown, formatTokenCount, formatTokensPerSec } from "../../lib/streamStats";

export function AgentTokenStats({
  stats,
  telemetry,
  live = false,
}: {
  stats: TokenStats;
  telemetry?: TurnTelemetry | null;
  live?: boolean;
}) {
  const approx = stats.source === "estimate";
  const tps = formatTokensPerSec(stats.tokensPerSec);
  const tokens =
    stats.completionTokens > 0
      ? approx
        ? `~${stats.completionTokens} tok`
        : `${stats.completionTokens} tok`
      : null;

  const cache = cacheBreakdown(stats);
  const cachePct = cache != null ? Math.round(cache.rate * 100) : null;
  const cacheTone =
    cachePct == null
      ? "text-gray-500"
      : cachePct >= 95
        ? "text-emerald-400"
        : cachePct >= 80
          ? "text-amber-300"
          : "text-red-300";

  return (
    <div
      className={`flex items-center gap-1.5 font-mono text-[10px] tabular-nums ${
        live ? "opacity-90" : "opacity-80"
      }`}
    >
      {live && (
        <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500/80" />
      )}
      <span className="text-gray-500">
        {approx ? "~" : ""}
        {tps}
        {tokens ? ` · ${tokens}` : ""}
      </span>
      {cache && cachePct != null ? (
        <span className={cacheTone} title="Provider prompt-cache hit / miss for this request">
          · {formatTokenCount(cache.hit)} hit · {formatTokenCount(cache.miss)} miss · {cachePct}%
        </span>
      ) : null}
      {telemetry && telemetry.toolCacheLookups > 0 ? (
        <span className="text-gray-500">
          · tools cache {Math.round(telemetry.toolCacheHitRate * 100)}%
        </span>
      ) : null}
    </div>
  );
}
