import { describe, expect, it } from "vitest";
import { catalogForProvider, catalogGroups, PROVIDER_CATALOG, searchCatalog } from "../../../lib/providerCatalog";

describe("provider catalog", () => {
  it("is sorted and grouped alphabetically A-Z", () => {
    const groups = catalogGroups();
    const letters = groups.map((g) => g.letter);
    expect(letters[0]).toBe("A");
    expect([...letters].sort().join("")).toBe(letters.join(""));
    // Every provider sits in its group.
    expect(groups.reduce((n, g) => n + g.providers.length, 0)).toBe(PROVIDER_CATALOG.length);
  });

  it("covers the majors from rick/opencode/pi/command-code", () => {
    const ids = PROVIDER_CATALOG.map((p) => p.id);
    for (const id of [
      "anthropic",
      "openai",
      "deepseek",
      "openrouter",
      "groq",
      "mistral",
      "gemini",
      "xai",
      "together",
      "ollama",
      "kilo",
      "kimi",
      "zai",
      "cerebras",
      "cohere",
      "perplexity",
      "github-copilot",
      "azure",
      "amazon-bedrock",
      "command-code",
      "cursor",
      "codex",
      "opencode",
      "nvidia",
    ]) {
      expect(ids).toContain(id);
    }
  });

  it("lists Kimi K3 and Llama models for NVIDIA NIM", () => {
    const nvidia = PROVIDER_CATALOG.find((p) => p.id === "nvidia");
    expect(nvidia).toBeDefined();
    expect(nvidia?.name).toBe("NVIDIA NIM");
    expect(nvidia?.kind).toBe("openai");
    expect(nvidia?.flavor).toBe("openai");
    expect(nvidia?.baseUrl).toBe("https://integrate.api.nvidia.com/v1");
    expect(nvidia?.defaultModel).toBe("moonshotai/kimi-k3");
    expect(nvidia?.models).toContain("moonshotai/kimi-k3");
    expect(nvidia?.models).toContain("meta/llama-3.3-70b-instruct");
  });

  it("lists Gemini models for Antigravity CLI (agy)", () => {
    const agy = PROVIDER_CATALOG.find((p) => p.id === "antigravity");
    expect(agy?.kind).toBe("antigravity_cli");
    expect(agy?.binPath).toBe("agy");
    expect(agy?.models.some((m) => m.startsWith("gemini-"))).toBe(true);
    expect(
      catalogForProvider({ kind: "antigravity_cli", name: "anything" })?.id,
    ).toBe("antigravity");
  });

  it("fuzzy-searches by name and id", () => {
    const hits = searchCatalog("deep");
    expect(hits.some((p) => p.id === "deepseek")).toBe(true);
    const groq = searchCatalog("grok");
    expect(groq.some((p) => p.id === "groq" || p.id === "xai")).toBe(true);
  });

  it("defines Kilo AI Gateway with OpenAI flavor and gateway base URL", () => {
    const kilo = PROVIDER_CATALOG.find((p) => p.id === "kilo");
    expect(kilo).toBeDefined();
    expect(kilo?.name).toBe("Kilo AI");
    expect(kilo?.kind).toBe("openai");
    expect(kilo?.flavor).toBe("openai");
    expect(kilo?.baseUrl).toBe("https://api.kilo.ai/api/gateway");
    expect(kilo?.needsKey).toBe(true);
    expect(kilo?.models.length).toBeGreaterThan(0);
  });

  it("every entry has a valid kind + flavor + base URL", () => {
    for (const p of PROVIDER_CATALOG) {
      expect(p.kind).toBeTruthy();
      expect(p.flavor).toBeTruthy();
      expect(p.baseUrl.length > 0).toBe(true);
      expect(Array.isArray(p.models)).toBe(true);
    }
  });
});
