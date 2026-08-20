import { describe, expect, it } from "vitest";
import {
  filterSlashCommands,
  isSlashInput,
  parseExactSlashCommand,
  slashQuery,
  SLASH_COMMANDS,
} from "./agentCommands";

describe("Agent Slash Commands", () => {
  it("identifies slash inputs correctly", () => {
    expect(isSlashInput("/new")).toBe(true);
    expect(isSlashInput("   /model")).toBe(true);
    expect(isSlashInput("hello /new")).toBe(false);
    expect(isSlashInput("")).toBe(false);
  });

  it("returns all slash commands on bare '/'", () => {
    const list = filterSlashCommands("/");
    expect(list).toEqual(SLASH_COMMANDS);
    expect(list.length).toBeGreaterThanOrEqual(7);
  });

  it("filters commands by prefix or description keyword", () => {
    const matches = filterSlashCommands("/mod");
    expect(matches.map((m) => m.name)).toContain("model");

    const historyMatches = filterSlashCommands("/hist");
    expect(historyMatches.map((m) => m.name)).toContain("history");

    const exportMatches = filterSlashCommands("/markdown");
    expect(exportMatches.map((m) => m.name)).toContain("export");
  });

  it("parses exact slash commands", () => {
    expect(parseExactSlashCommand("/new")?.actionKey).toBe("new");
    expect(parseExactSlashCommand("/plan")?.actionKey).toBe("plan");
    expect(parseExactSlashCommand("/compact")?.actionKey).toBe("compact");
    expect(parseExactSlashCommand("/vision")?.actionKey).toBe("vision");
    expect(parseExactSlashCommand("/close")?.actionKey).toBe("close");
    expect(parseExactSlashCommand("/hide")?.actionKey).toBe("close");
    expect(parseExactSlashCommand("/prices")?.actionKey).toBe("prices");
    expect(parseExactSlashCommand("/mode")?.actionKey).toBe("mode");
    expect(parseExactSlashCommand("/trajectory")?.actionKey).toBe("trajectory");
    expect(parseExactSlashCommand("/goal")?.actionKey).toBe("goal");
    expect(parseExactSlashCommand("/goal rank my site")).toBeNull();
    expect(parseExactSlashCommand("/unknown")).toBeNull();
  });

  it("does not intercept /goal or /loop once arguments are typed", () => {
    expect(slashQuery("/goal rank my site")).toEqual({
      name: "goal",
      rest: "rank my site",
    });
    expect(filterSlashCommands("/goal rank my site")).toEqual([]);
    expect(filterSlashCommands("/goal ")).toEqual([]);
    expect(filterSlashCommands("/goal").map((c) => c.name)).toContain("goal");
    expect(SLASH_COMMANDS.find((c) => c.name === "goal")?.needsArg).toBe(true);
  });

  it("finds /vision by name and description", () => {
    expect(filterSlashCommands("/vis").map((m) => m.name)).toContain("vision");
    expect(filterSlashCommands("/gemini").map((m) => m.name)).toContain("vision");
  });
});
