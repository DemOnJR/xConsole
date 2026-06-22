/** Rough token estimate from streamed text (~4 chars per token). */
export function estimateTokens(text: string): number {
  if (!text) return 0;
  return Math.max(1, Math.ceil(text.length / 4));
}

export interface TokenStats {
  completionTokens: number;
  promptTokens?: number;
  tokensPerSec: number;
  source: "estimate" | "provider";
}

export interface ContextUsageSegment {
  key: string;
  label: string;
  tokens: number;
}

export interface ContextUsage {
  segments: ContextUsageSegment[];
  total_tokens: number;
  context_limit: number;
  percent: number;
}

export function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 10_000) return `${Math.round(n / 1000)}K`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return String(n);
}

export function formatTokensPerSec(tps: number): string {
  if (!Number.isFinite(tps) || tps <= 0) return "—";
  if (tps >= 100) return `${Math.round(tps)} tok/s`;
  if (tps >= 10) return `${tps.toFixed(1)} tok/s`;
  return `${tps.toFixed(2)} tok/s`;
}

export function liveTokenStats(text: string, startedAtMs: number): TokenStats {
  const elapsedSec = Math.max((Date.now() - startedAtMs) / 1000, 0.05);
  const completionTokens = estimateTokens(text);
  return {
    completionTokens,
    tokensPerSec: completionTokens / elapsedSec,
    source: "estimate",
  };
}
