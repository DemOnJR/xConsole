import { describe, expect, it } from "vitest";
import { PROVIDER_PRESETS } from "./ProvidersSection";

describe("Command Code provider presets", () => {
  it("defines the DeepSeek V4 Flash OpenAI-compatible preset without secrets", () => {
    const preset = PROVIDER_PRESETS.find((item) => item.id === "commandcode-deepseek-v4-flash");
    expect(preset).toEqual({
      id: "commandcode-deepseek-v4-flash",
      label: "Command Code · DeepSeek V4 Flash",
      kind: "openai",
      base_url: "https://api.commandcode.ai/provider/v1",
      model: "deepseek/deepseek-v4-flash",
    });
    expect(preset).not.toHaveProperty("secret");
  });

  it("keeps the existing Command Code Claude preset unchanged", () => {
    expect(PROVIDER_PRESETS.find((item) => item.id === "commandcode")).toEqual({
      id: "commandcode",
      label: "Command Code",
      kind: "openai",
      base_url: "https://api.commandcode.ai/provider/v1",
      model: "anthropic/claude-sonnet-4-5",
    });
  });

  it("defines the Antigravity CLI preset", () => {
    expect(PROVIDER_PRESETS.find((item) => item.id === "antigravity")).toEqual({
      id: "antigravity",
      label: "Antigravity CLI (agy)",
      kind: "antigravity_cli",
      base_url: "",
      model: "agent",
    });
  });
});
