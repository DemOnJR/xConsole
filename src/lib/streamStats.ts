/** Rough token estimate from streamed text (~4 chars per token). */
export function estimateTokens(text: string): number {
  if (!text) return 0;
  return Math.max(1, Math.ceil(text.length / 4));
}

export interface TokenStats {
  completionTokens: number;
  promptTokens?: number;
  /** Provider prompt-cache hits (Anthropic cache_read_input_tokens, etc.). */
  cachedTokens?: number;
  /** Provider prompt-cache writes (cache_creation_input_tokens). */
  cacheCreationTokens?: number;
  /** Estimated USD for this turn (from the backend price table). */
  costUsd?: number;
  tokensPerSec: number;
  source: "estimate" | "provider";
}

/** Cache hit rate 0..1: reads / (reads + fresh input). */
export function cacheHitRate(stats: TokenStats): number | null {
  if (stats.source !== "provider") return null;
  const total = (stats.cachedTokens ?? 0) + (stats.promptTokens ?? 0);
  if (total <= 0) return null;
  return (stats.cachedTokens ?? 0) / total;
}

export function formatUsd(n: number | undefined): string {
  if (n === undefined || !Number.isFinite(n) || n <= 0) return "";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  return `$${n.toFixed(3)}`;
}

export interface TurnTelemetry {
  toolCalls: number;
  toolCacheLookups: number;
  toolCacheHits: number;
  toolCacheMisses: number;
  toolCacheWrites: number;
  toolCacheHitRate: number;
}

export interface PrefixTelemetry {
  requestIndex: number;
  systemHash: string;
  schemaHash: string;
  messagePrefixHash: string;
  systemBytes: number;
  schemaBytes: number;
  messageBytes: number;
  classification: string;
  source: string;
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
