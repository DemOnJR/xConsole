import type { ContextUsage, PrefixTelemetry, TokenStats, TurnTelemetry } from "../../lib/streamStats";
import { cacheHitRate, formatTokensPerSec, formatUsd } from "../../lib/streamStats";
import type { AiProvider } from "../../lib/tauri";

export function AgentConsoleFooter({
  activeProvider,
  targetsCount,
  planMode,
  streaming,
  streamStats,
  turnTelemetry,
  prefixTelemetry,
  contextUsage,
  conversationCostUsd,
  onTogglePlanMode,
  onOpenSettings,
  onStop,
}: {
  activeProvider?: AiProvider;
  targetsCount: number;
  planMode: boolean;
  streaming: boolean;
  streamStats: TokenStats | null;
  turnTelemetry: TurnTelemetry | null;
  prefixTelemetry: PrefixTelemetry | null;
  contextUsage: ContextUsage | null;
  conversationCostUsd: number;
  onTogglePlanMode: () => void;
  onOpenSettings: (section: string) => void;
  onStop: () => void;
}) {
  const modelLabel = activeProvider?.model || activeProvider?.name || "No model";
  const tps = streamStats ? formatTokensPerSec(streamStats.tokensPerSec) : null;
  const hitRate = streamStats ? cacheHitRate(streamStats) : null;
  const cost = formatUsd(streamStats?.costUsd);
  const totalCost = formatUsd(conversationCostUsd > 0 ? conversationCostUsd : undefined);

  return (
    <div className="flex select-none items-center justify-between border-t border-[var(--border)] bg-[var(--surface-muted)] px-3 py-1 text-[10px] text-[var(--text-dim)] font-mono">
      {/* Left side: Model & Targets */}
      <div className="flex items-center gap-2 overflow-hidden">
        <button
          type="button"
          onClick={() => onOpenSettings("providers")}
          className="truncate rounded px-1 py-0.5 text-gray-300 transition hover:bg-[var(--border)] hover:text-white"
          title="Configure AI provider"
        >
          {activeProvider?.kind ? `${activeProvider.kind}:` : ""}{modelLabel}
        </button>

        <span className="text-gray-600">·</span>

        <span className="truncate text-gray-400">
          {targetsCount === 0 ? "infra mode" : `${targetsCount} target${targetsCount > 1 ? "s" : ""}`}
        </span>

        <button
          type="button"
          onClick={onTogglePlanMode}
          className={`rounded px-1.5 py-0.5 text-[9px] uppercase font-bold tracking-wider transition ${
            planMode
              ? "bg-indigo-500/20 text-indigo-300 ring-1 ring-indigo-500/40"
              : "text-gray-500 hover:text-gray-300"
          }`}
          title="Toggle Plan Mode (reconstruct plan before executing)"
        >
          Plan
        </button>
      </div>

      {/* Right side: Live telemetry, context %, Stop button */}
      <div className="flex items-center gap-2 shrink-0">
        {contextUsage && (
          <span className="text-gray-400" title={`Context: ${contextUsage.total_tokens.toLocaleString()} / ${contextUsage.context_limit.toLocaleString()} tokens`}>
            {contextUsage.percent}% ctx
          </span>
        )}

        {prefixTelemetry && prefixTelemetry.classification !== "append_only" && (
          <span className="text-[9px] text-gray-500">
            {prefixTelemetry.classification}
          </span>
        )}

        {streamStats && (
          <span className="text-emerald-400">
            {tps}
            {hitRate != null ? ` · ${Math.round(hitRate * 100)}% cached` : ""}
            {cost ? ` · ${cost}` : ""}
            {totalCost ? ` · ${totalCost} tot` : ""}
            {turnTelemetry && turnTelemetry.toolCacheLookups > 0
              ? ` · ${Math.round(turnTelemetry.toolCacheHitRate * 100)}% tools`
              : ""}
          </span>
        )}

        {streaming ? (
          <button
            type="button"
            onClick={onStop}
            className="flex items-center gap-1 rounded bg-red-950/60 px-1.5 py-0.5 text-red-300 ring-1 ring-red-500/30 transition hover:bg-red-900/80 hover:text-white"
          >
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-red-400" />
            <span>Stop (Esc)</span>
          </button>
        ) : (
          <span className="text-[9px] text-gray-500">Ready</span>
        )}
      </div>
    </div>
  );
}
