import { describe, expect, it } from "vitest";
import { resolveEffectiveMode } from "./agentMode";

describe("resolveEffectiveMode", () => {
  it("respects explicit modes without modifying them", () => {
    expect(resolveEffectiveMode("code", "please plan the database")).toBe("code");
    expect(resolveEffectiveMode("plan", "write a python function")).toBe("plan");
    expect(resolveEffectiveMode("minimal", "complex architecture")).toBe("minimal");
    expect(resolveEffectiveMode("standard", "hello")).toBe("standard");
  });

  it("auto-detects planning intent in English and Romanian", () => {
    expect(resolveEffectiveMode("auto", "Can you make a plan for moving to Postgres?")).toBe("plan");
    expect(resolveEffectiveMode("auto", "Propose a plan before touching any files")).toBe("plan");
    expect(resolveEffectiveMode("auto", "cum facem sa migram serverul?")).toBe("plan");
    expect(resolveEffectiveMode("auto", "fa un plan pas cu pas")).toBe("plan");
    expect(resolveEffectiveMode("auto", "Analyze architecture and roadmap")).toBe("plan");
  });

  it("auto-detects direct coding intent", () => {
    expect(resolveEffectiveMode("auto", "Write code for a React modal component")).toBe("code");
    expect(resolveEffectiveMode("auto", "Fix the bug in auth.rs")).toBe("code");
    expect(resolveEffectiveMode("auto", "Refactor this TypeScript store")).toBe("code");
    expect(resolveEffectiveMode("auto", "Scrie cod pentru api endpoint")).toBe("code");
    expect(resolveEffectiveMode("auto", "Implement user registration form")).toBe("code");
  });

  it("auto-detects minimal intent on short casual greetings", () => {
    expect(resolveEffectiveMode("auto", "hi")).toBe("minimal");
    expect(resolveEffectiveMode("auto", "salut")).toBe("minimal");
    expect(resolveEffectiveMode("auto", "thanks!")).toBe("minimal");
    expect(resolveEffectiveMode("auto", "multumesc")).toBe("minimal");
  });

  it("falls back to standard on general or empty prompts", () => {
    expect(resolveEffectiveMode("auto", "")).toBe("standard");
    expect(resolveEffectiveMode("auto", "What is the status of the docker containers?")).toBe("standard");
    expect(resolveEffectiveMode("auto", "Check disk usage and memory on web01")).toBe("standard");
  });
});
