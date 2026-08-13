import { describe, expect, it } from "vitest";
import { parseGoalMemory, parseGoalSpec, parseGoalTasks } from "./goalParse";

describe("goal JSON parsers", () => {
  it("treats the empty intake spec as missing, not a crash", () => {
    expect(parseGoalSpec("{}")).toBeNull();
    expect(parseGoalSpec("")).toBeNull();
    expect(parseGoalSpec("not json")).toBeNull();
  });

  it("reads a locked spec without requiring every field", () => {
    const spec = parseGoalSpec(
      JSON.stringify({
        objective: "watch k8s",
        success_criteria: ["no new attacks"],
      }),
    );
    expect(spec?.objective).toBe("watch k8s");
    expect(spec?.success_criteria).toEqual(["no new attacks"]);
    expect(spec?.check_method).toBe("");
    expect(spec?.hard_constraints).toEqual([]);
  });

  it("never leaves memory.learned undefined", () => {
    expect(parseGoalMemory("{}").learned).toEqual([]);
    expect(parseGoalMemory("").learned).toEqual([]);
    expect(parseGoalMemory("{").learned).toEqual([]);
    expect(parseGoalMemory(JSON.stringify({ learned: [{ key: "ttl", value: "3d" }] })).learned).toEqual(
      [{ key: "ttl", value: "3d", evidence: "", confidence: "" }],
    );
  });

  it("normalizes a broken kanban payload", () => {
    expect(parseGoalTasks("{}")).toEqual([]);
    expect(parseGoalTasks(JSON.stringify([{ title: "scan" }]))[0]).toMatchObject({
      id: "task-0",
      column: "backlog",
      title: "scan",
    });
  });
});
