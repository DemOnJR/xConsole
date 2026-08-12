import { describe, expect, it } from "vitest";
import { cacheHitRate, formatUsd, formatTokenCount } from "./streamStats";

describe("cacheHitRate", () => {
  it("computes reads / (reads + fresh input) for provider stats", () => {
    const rate = cacheHitRate({
      completionTokens: 100,
      promptTokens: 1000,
      cachedTokens: 39_000,
      tokensPerSec: 1,
      source: "provider",
    });
    expect(rate).toBeCloseTo(39_000 / 40_000);
  });

  it("returns null for estimates or missing data", () => {
    expect(cacheHitRate({ completionTokens: 1, tokensPerSec: 1, source: "estimate" })).toBeNull();
    expect(
      cacheHitRate({ completionTokens: 1, promptTokens: 0, cachedTokens: 0, tokensPerSec: 1, source: "provider" }),
    ).toBeNull();
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

describe("formatTokenCount", () => {
  it("keeps existing behavior", () => {
    expect(formatTokenCount(500)).toBe("500");
    expect(formatTokenCount(12_000)).toBe("12K");
    expect(formatTokenCount(1_500_000)).toBe("1.5M");
  });
});
