import type { TokenStats, TurnTelemetry } from "../../lib/streamStats";
import { formatTokensPerSec } from "../../lib/streamStats";

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

  const cachePct =
    stats.cachedTokens != null &&
    stats.promptTokens != null &&
    stats.promptTokens > 0
      ? Math.round((stats.cachedTokens / stats.promptTokens) * 100)
      : null;

  return (
    <div
      className={`flex items-center gap-1.5 font-mono text-[10px] tabular-nums text-gray-500 ${
        live ? "opacity-90" : "opacity-80"
      }`}
    >
      {live && (
        <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500/80" />
      )}
      <span>
        {approx ? "~" : ""}
        {tps}
        {tokens ? ` · ${tokens}` : ""}
        {cachePct != null ? ` · cache ${cachePct}%` : ""}
        {telemetry && telemetry.toolCacheLookups > 0
          ? ` · tools cache ${Math.round(telemetry.toolCacheHitRate * 100)}%`
          : ""}
      </span>
    </div>
  );
}
