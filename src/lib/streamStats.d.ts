/** Rough token estimate from streamed text (~4 chars per token). */
export declare function estimateTokens(text: string): number;
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
export declare function cacheBreakdown(stats: TokenStats): CacheBreakdown | null;
/** Cache hit rate 0..1, or null when the provider did not report usage. */
export declare function cacheHitRate(stats: TokenStats): number | null;
export declare function formatCacheLine(stats: TokenStats): string;
/** Running prompt-cache totals for a conversation (sum of every provider turn). */
export interface SessionCacheTotals {
    hit: number;
    miss: number;
    turns: number;
    rate: number;
}
export declare function emptySessionCache(): SessionCacheTotals;
export declare function addTurnToSessionCache(acc: SessionCacheTotals, stats: TokenStats | null | undefined): SessionCacheTotals;
export declare function sessionCacheFromMessages(messages: {
    tokenStats?: TokenStats;
}[]): SessionCacheTotals;
export declare function sessionCostFromMessages(messages: {
    tokenStats?: TokenStats;
}[]): number;
/** Prefer live provider usage; if the stream is still an estimate, keep the last reply's cache. */
export declare function displayTurnStats(live: TokenStats | null | undefined, last: TokenStats | null | undefined): TokenStats | null;
export declare function formatSessionCache(acc: SessionCacheTotals): string;
/** Multi-line hover copy for the compact cache meter. */
export declare function formatCacheTooltip(stats: TokenStats | null | undefined, session?: SessionCacheTotals | null, costUsd?: number): string;
export declare function formatUsd(n: number | undefined): string;
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
export declare function formatTokenCount(n: number): string;
export declare function formatTokensPerSec(tps: number): string;
export declare function liveTokenStats(text: string, startedAtMs: number): TokenStats;
/** tok/s for the current generation burst only (excludes earlier tool-wait time). */
export declare function liveGenerationStats(fullText: string, burstStartedAtMs: number, tokensBeforeBurst: number): TokenStats;
export declare function defaultContextLimit(kind?: string, model?: string): number;
export declare function contextUsageFromMessages(messages: Array<{
    role: string;
    content?: string;
    toolCalls?: Array<{
        name: string;
        arguments?: unknown;
    }>;
    images?: unknown[];
    tokenStats?: TokenStats;
}>, providerKind?: string, model?: string): ContextUsage | null;
//# sourceMappingURL=streamStats.d.ts.map