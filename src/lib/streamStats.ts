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

export interface CacheBreakdown {
  hit: number;
  miss: number;
  rate: number;
}

/**
 * Hit / miss token counts from provider-reported prompt + cached.
 *
 * OpenAI / DeepSeek / Command Code: `promptTokens` is inclusive (cached is a
 * subset). Anthropic: `promptTokens` is cache-miss only.
 */
export function cacheBreakdown(stats: TokenStats): CacheBreakdown | null {
  if (stats.source !== "provider") return null;
  const prompt = stats.promptTokens ?? 0;
  const cached = stats.cachedTokens ?? 0;
  if (cached > 0 && cached <= prompt) {
    return { hit: cached, miss: prompt - cached, rate: cached / prompt };
  }
  const total = cached + prompt;
  if (total <= 0) return null;
  return { hit: cached, miss: prompt, rate: cached / total };
}

/** Cache hit rate 0..1, or null when the provider did not report usage. */
export function cacheHitRate(stats: TokenStats): number | null {
  return cacheBreakdown(stats)?.rate ?? null;
}

export function formatCacheLine(stats: TokenStats): string {
  const b = cacheBreakdown(stats);
  if (!b) return "";
  return `cache ${formatTokenCount(b.hit)} hit · ${formatTokenCount(b.miss)} miss · ${Math.round(b.rate * 100)}%`;
}

/** Running prompt-cache totals for a conversation (sum of every provider turn). */
export interface SessionCacheTotals {
  hit: number;
  miss: number;
  turns: number;
  rate: number;
}

export function emptySessionCache(): SessionCacheTotals {
  return { hit: 0, miss: 0, turns: 0, rate: 0 };
}

export function addTurnToSessionCache(
  acc: SessionCacheTotals,
  stats: TokenStats | null | undefined,
): SessionCacheTotals {
  const b = stats ? cacheBreakdown(stats) : null;
  if (!b) return acc;
  const hit = acc.hit + b.hit;
  const miss = acc.miss + b.miss;
  const turns = acc.turns + 1;
  const total = hit + miss;
  return { hit, miss, turns, rate: total > 0 ? hit / total : 0 };
}

export function sessionCacheFromMessages(
  messages: { tokenStats?: TokenStats }[],
): SessionCacheTotals {
  return messages.reduce(
    (acc, message) => addTurnToSessionCache(acc, message.tokenStats),
    emptySessionCache(),
  );
}

export function sessionCostFromMessages(messages: { tokenStats?: TokenStats }[]): number {
  return messages.reduce((sum, message) => sum + (message.tokenStats?.costUsd ?? 0), 0);
}

/** Prefer live provider usage; if the stream is still an estimate, keep the last reply's cache. */
export function displayTurnStats(
  live: TokenStats | null | undefined,
  last: TokenStats | null | undefined,
): TokenStats | null {
  if (live?.source === "provider") return live;
  if (live && last && cacheBreakdown(last)) {
    return {
      ...last,
      completionTokens: live.completionTokens,
      tokensPerSec: live.tokensPerSec,
    };
  }
  return live ?? last ?? null;
}

export function formatSessionCache(acc: SessionCacheTotals): string {
  if (acc.turns <= 0) return "";
  const turns = acc.turns === 1 ? "1 turn" : `${acc.turns} turns`;
  return `session ${formatTokenCount(acc.hit)} hit · ${formatTokenCount(acc.miss)} miss · ${Math.round(acc.rate * 100)}% avg · ${turns}`;
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
