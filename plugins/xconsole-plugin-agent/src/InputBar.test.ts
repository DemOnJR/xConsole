import { describe, expect, it } from "vitest";
import { reasoningCapable } from "./InputBar";

describe("reasoningCapable", () => {
  it("enables reasoning for Anthropic always", () => {
    expect(reasoningCapable("anthropic", "claude-sonnet-4-5")).toBe(true);
    expect(reasoningCapable("anthropic", undefined)).toBe(true);
  });

  it("enables reasoning for Ollama (think)", () => {
    expect(reasoningCapable("ollama", "qwen3.5:9b")).toBe(true);
  });

  it("enables reasoning for OpenAI-compatible models", () => {
    expect(reasoningCapable("openai", "gpt-5")).toBe(true);
    expect(reasoningCapable("openai", "gpt-4o")).toBe(true);
  });

  it("disables reasoning for CLI kinds", () => {
    expect(reasoningCapable("cursor", "auto")).toBe(false);
    expect(reasoningCapable("codex_cli", undefined)).toBe(false);
    expect(reasoningCapable("opencode_cli", undefined)).toBe(false);
    expect(reasoningCapable("antigravity_cli", "agent")).toBe(false);
    expect(reasoningCapable(undefined, undefined)).toBe(false);
  });
});
