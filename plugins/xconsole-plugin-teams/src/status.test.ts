import { describe, expect, it } from "vitest";
import { goalPhase, memberLive } from "./status";
import type { GoalSession, Persona } from "../../../src/lib/tauri";

const persona = (partial: Partial<Persona> & Pick<Persona, "id" | "name">): Persona => ({
  role: "engineer",
  instructions: "",
  targets: [],
  enabled: true,
  ...partial,
});

const goal = (partial: Partial<GoalSession> & Pick<GoalSession, "id" | "title" | "status">): GoalSession => ({
  raw_request: "",
  spec_json: "{}",
  kanban_json: "[]",
  memory_json: "{}",
  cycles: 1,
  ...partial,
});

describe("goalPhase", () => {
  it("maps the loop states the teams list shows", () => {
    expect(goalPhase("active")).toBe("working");
    expect(goalPhase("intake")).toBe("planning");
    expect(goalPhase("waiting")).toBe("waiting");
    expect(goalPhase("blocked")).toBe("blocked");
    expect(goalPhase("done")).toBe("idle");
  });
});

describe("memberLive", () => {
  it("prefers the live tool sentence over a generic Working", () => {
    const ada = persona({ id: "ada", name: "Ada" });
    const live = {
      ada: {
        persona_id: "ada",
        session_id: "s",
        workspace_id: null,
        status: "working",
        detail: "Reading /etc/hosts",
        updatedAt: Date.now(),
      },
    };
    const got = memberLive(ada, live, []);
    expect(got.phase).toBe("working");
    expect(got.label).toBe("Reading /etc/hosts");
  });

  it("falls back to the running goal when nothing is in flight", () => {
    const ada = persona({ id: "ada", name: "Ada" });
    const got = memberLive(ada, {}, [
      goal({ id: "g1", title: "Rotate Redis", status: "active", persona_id: "ada" }),
    ]);
    expect(got.phase).toBe("working");
    expect(got.task).toBe("Rotate Redis");
    expect(got.label).toBe("Working");
  });

  it("is idle when the agent has no live turn and no open task", () => {
    const ada = persona({ id: "ada", name: "Ada" });
    expect(memberLive(ada, {}, []).phase).toBe("idle");
  });
});
