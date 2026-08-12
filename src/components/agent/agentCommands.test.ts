import { describe, expect, it } from "vitest";
import {
  filterSlashCommands,
  isSlashInput,
  parseExactSlashCommand,
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
    expect(parseExactSlashCommand("/unknown")).toBeNull();
  });
});
