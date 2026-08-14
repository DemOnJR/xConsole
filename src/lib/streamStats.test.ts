import { describe, expect, it } from "vitest";
import {
  addTurnToSessionCache,
  cacheBreakdown,
  cacheHitRate,
  displayTurnStats,
  emptySessionCache,
  formatCacheLine,
  formatCacheTooltip,
  formatSessionCache,
  formatUsd,
  formatTokenCount,
  liveGenerationStats,
  sessionCacheFromMessages,
  sessionCostFromMessages,
} from "./streamStats";

describe("cacheHitRate", () => {
  it("uses exclusive (Anthropic) math when cached > prompt", () => {
    const rate = cacheHitRate({
      completionTokens: 100,
      promptTokens: 1000,
      cachedTokens: 39_000,
      tokensPerSec: 1,
      source: "provider",
    });
    expect(rate).toBeCloseTo(39_000 / 40_000);
  });

  it("uses inclusive (DeepSeek / OpenAI / Command Code) math when cached ⊆ prompt", () => {
    const rate = cacheHitRate({
      completionTokens: 50,
      promptTokens: 50_000,
      cachedTokens: 48_000,
      tokensPerSec: 1,
      source: "provider",
    });
    expect(rate).toBeCloseTo(0.96);
  });

  it("returns null for estimates or missing data", () => {
    expect(cacheHitRate({ completionTokens: 1, tokensPerSec: 1, source: "estimate" })).toBeNull();
    expect(
      cacheHitRate({ completionTokens: 1, promptTokens: 0, cachedTokens: 0, tokensPerSec: 1, source: "provider" }),
    ).toBeNull();
  });

  it("splits hit and miss tokens for the terminal line", () => {
    const b = cacheBreakdown({
      completionTokens: 16,
      promptTokens: 1732,
      cachedTokens: 1664,
      tokensPerSec: 1,
      source: "provider",
    });
    expect(b).toEqual({ hit: 1664, miss: 68, rate: 1664 / 1732 });
    expect(
      formatCacheLine({
        completionTokens: 16,
        promptTokens: 1732,
        cachedTokens: 1664,
        tokensPerSec: 1,
        source: "provider",
      }),
    ).toBe("cache 1.7K hit · 68 miss · 96%");
  });
});

describe("formatCacheTooltip", () => {
  it("stacks tok/s, tokens, and cache on separate hover lines", () => {
    const text = formatCacheTooltip(
      {
        completionTokens: 4096,
        tokensPerSec: 96,
        promptTokens: 2000,
        cachedTokens: 18000,
        source: "provider",
      },
      { hit: 18000, miss: 1300, turns: 2, rate: 18000 / 19300 },
      0.0123,
    );
    expect(text).toContain("96.0 tok/s");
    expect(text).toContain("4096 tok");
    expect(text).toContain("93%");
    expect(text).toContain("session");
    expect(text).toContain("$0.0123");
  });
});

describe("liveGenerationStats", () => {
  it("rates only the new burst, not earlier text", () => {
    const started = Date.now() - 1000;
    const s = liveGenerationStats("xxxx".repeat(50), started, 10);
    // 200 chars ≈ 50 tokens; 50 - 10 = 40 new tokens in ~1s
    expect(s.completionTokens).toBe(50);
    expect(s.tokensPerSec).toBeGreaterThan(20);
    expect(s.source).toBe("estimate");
  });
});

describe("formatUsd", () => {
  it("formats small and large amounts", () => {
    expect(formatUsd(0.0123)).toBe("$0.012");
    expect(formatUsd(0.0004)).toBe("$0.0004");
    expect(formatUsd(1.5)).toBe("$1.500");
  });

  it("returns empty for missing/zero/non-finite", () => {
    expect(formatUsd(undefined)).toBe("");
    expect(formatUsd(0)).toBe("");
    expect(formatUsd(NaN)).toBe("");
  });
});

describe("session cache totals", () => {
  const turn = (prompt: number, cached: number, costUsd = 0) => ({
    completionTokens: 10,
    promptTokens: prompt,
    cachedTokens: cached,
    costUsd,
    tokensPerSec: 1,
    source: "provider" as const,
  });

  it("sums hit/miss across replies and averages the rate", () => {
    const acc = sessionCacheFromMessages([
      { tokenStats: turn(1732, 1664) },
      { tokenStats: turn(50_000, 48_000) },
      {},
    ]);
    expect(acc.turns).toBe(2);
    expect(acc.hit).toBe(1664 + 48_000);
    expect(acc.miss).toBe(68 + 2000);
    expect(acc.rate).toBeCloseTo((1664 + 48_000) / (1732 + 50_000));
    expect(formatSessionCache(acc)).toBe("session 50K hit · 2.1K miss · 96% avg · 2 turns");
  });

  it("ignores estimate-only turns so a live stream does not reset the session", () => {
    const first = addTurnToSessionCache(emptySessionCache(), turn(1732, 1664));
    const still = addTurnToSessionCache(first, {
      completionTokens: 40,
      tokensPerSec: 12,
      source: "estimate",
    });
    expect(still).toEqual(first);
  });

  it("keeps the last reply's cache while the next stream is still an estimate", () => {
    const last = turn(1732, 1664);
    const live = { completionTokens: 40, tokensPerSec: 22, source: "estimate" as const };
    const shown = displayTurnStats(live, last);
    expect(cacheBreakdown(shown!)?.hit).toBe(1664);
    expect(shown?.tokensPerSec).toBe(22);
    expect(displayTurnStats(turn(50_000, 48_000), last)?.cachedTokens).toBe(48_000);
  });

  it("sums stored per-reply cost", () => {
    expect(
      sessionCostFromMessages([
        { tokenStats: turn(100, 80, 0.0012) },
        { tokenStats: turn(200, 180, 0.003) },
      ]),
    ).toBeCloseTo(0.0042);
  });
});

describe("formatTokenCount", () => {
  it("keeps existing behavior", () => {
    expect(formatTokenCount(500)).toBe("500");
    expect(formatTokenCount(12_000)).toBe("12K");
    expect(formatTokenCount(1_500_000)).toBe("1.5M");
  });
});
