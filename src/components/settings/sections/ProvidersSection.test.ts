import { describe, expect, it } from "vitest";
import { PROVIDER_PRESETS } from "./ProvidersSection";

describe("Command Code provider presets", () => {
  it("defines the default Command Code preset with DeepSeek V4 Flash", () => {
    const preset = PROVIDER_PRESETS.find((item) => item.id === "commandcode");
    expect(preset).toEqual({
      id: "commandcode",
      label: "Command Code",
      kind: "openai",
      base_url: "https://api.commandcode.ai/provider/v1",
      model: "deepseek/deepseek-v4-flash",
    });
    expect(preset).not.toHaveProperty("secret");
  });

  it("defines the Command Code Claude preset", () => {
    expect(PROVIDER_PRESETS.find((item) => item.id === "commandcode-claude")).toEqual({
      id: "commandcode-claude",
      label: "Command Code (Claude)",
      kind: "openai",
      base_url: "https://api.commandcode.ai/provider/v1",
      model: "anthropic/claude-sonnet-4-5",
    });
  });

  it("defines the NVIDIA NIM preset", () => {
    expect(PROVIDER_PRESETS.find((item) => item.id === "nvidia")).toEqual({
      id: "nvidia",
      label: "NVIDIA NIM",
      kind: "openai",
      base_url: "https://integrate.api.nvidia.com/v1",
      model: "moonshotai/kimi-k3",
    });
  });

  it("defines the Antigravity CLI preset", () => {
    expect(PROVIDER_PRESETS.find((item) => item.id === "antigravity")).toEqual({
      id: "antigravity",
      label: "Antigravity CLI (agy)",
      kind: "antigravity_cli",
      base_url: "",
      model: "gemini-3.7-flash-high",
    });
  });
});
