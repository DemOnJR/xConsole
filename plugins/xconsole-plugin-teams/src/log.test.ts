import { describe, expect, it } from "vitest";
import type { AgentLogEntry } from "../../../src/lib/tauri";
import { logText, mergeLog, type LiveLogLine } from "./log";

const entry = (
  partial: Partial<AgentLogEntry> & Pick<AgentLogEntry, "id" | "persona_id">,
): AgentLogEntry => ({
  session_id: "",
  status: "working",
  detail: "",
  ...partial,
});

const at = (iso: string) => Date.parse(iso);

const live = (partial: Partial<LiveLogLine> & Pick<LiveLogLine, "at">): LiveLogLine => ({
  personaId: "ada",
  status: "working",
  detail: "",
  ...partial,
});

describe("mergeLog", () => {
  it("interleaves persisted rows and the live tail in timestamp order", () => {
    const merged = mergeLog(
      [
        entry({ id: "l1", persona_id: "ada", detail: "first", created_at: "2026-09-01 16:00:00" }),
        entry({ id: "l3", persona_id: "ada", detail: "third", created_at: "2026-09-01 16:02:00" }),
      ],
      [live({ detail: "second", at: at("2026-09-01T16:01:00Z") })],
    );
    expect(merged.map((l) => l.detail)).toEqual(["first", "second", "third"]);
    expect(merged.map((l) => l.live)).toEqual([false, true, false]);
  });

  it("shows one line for an action that arrived by both paths", () => {
    // The same event: seen live as it happened, then read back from agent_log on the
    // next reload. Two ids, two timestamps, one thing that happened.
    const merged = mergeLog(
      [
        entry({
          id: "l1",
          persona_id: "ada",
          tool: "run_command",
          detail: "kubectl get pods",
          created_at: "2026-09-01 16:00:00",
        }),
      ],
      [
        live({
          tool: "run_command",
          detail: "kubectl get pods",
          at: at("2026-09-01T16:00:02Z"),
        }),
      ],
    );
    expect(merged).toHaveLength(1);
    // The persisted row wins, because its id survives a restart and anchors a thread.
    expect(merged[0].id).toBe("l1");
    expect(merged[0].live).toBe(false);
  });

  it("keeps the same words said much later as a separate line", () => {
    const merged = mergeLog(
      [
        entry({
          id: "l1",
          persona_id: "ada",
          detail: "kubectl get pods",
          created_at: "2026-09-01 16:00:00",
        }),
      ],
      [live({ detail: "kubectl get pods", at: at("2026-09-01T16:30:00Z") })],
    );
    expect(merged).toHaveLength(2);
  });

  it("does not merge two agents saying the same thing at the same moment", () => {
    const merged = mergeLog(
      [
        entry({
          id: "l1",
          persona_id: "ada",
          detail: "kubectl get pods",
          created_at: "2026-09-01 16:00:00",
        }),
      ],
      [
        live({
          personaId: "bruno",
          detail: "kubectl get pods",
          at: at("2026-09-01T16:00:00Z"),
        }),
      ],
    );
    expect(merged).toHaveLength(2);
  });

  it("collapses consecutive identical tool lines into one with a count", () => {
    const merged = mergeLog(
      [
        entry({ id: "a", persona_id: "ada", tool: "read_file", detail: "main.rs", created_at: "2026-09-01 16:00:00" }),
        entry({ id: "b", persona_id: "ada", tool: "read_file", detail: "main.rs", created_at: "2026-09-01 16:00:10" }),
        entry({ id: "c", persona_id: "ada", tool: "read_file", detail: "main.rs", created_at: "2026-09-01 16:00:20" }),
        entry({ id: "d", persona_id: "ada", tool: "run_command", detail: "cargo test", created_at: "2026-09-01 16:00:30" }),
      ],
      [],
    );
    expect(merged.map((l) => [l.id, l.repeat])).toEqual([
      ["a", 3],
      ["d", 1],
    ]);
    // The anchor is the first occurrence, so a thread opened on it stays put as more
    // repeats arrive.
    expect(merged[0].id).toBe("a");
  });

  it("does not collapse identical lines that are not next to each other", () => {
    const merged = mergeLog(
      [
        entry({ id: "a", persona_id: "ada", detail: "x", created_at: "2026-09-01 16:00:00" }),
        entry({ id: "b", persona_id: "ada", detail: "y", created_at: "2026-09-01 16:00:10" }),
        entry({ id: "c", persona_id: "ada", detail: "x", created_at: "2026-09-01 16:00:20" }),
      ],
      [],
    );
    expect(merged.map((l) => l.detail)).toEqual(["x", "y", "x"]);
  });

  it("gives a live-only line a stable id so a thread can hang off it", () => {
    const one = mergeLog([], [live({ detail: "thinking", at: 1234 })]);
    const again = mergeLog([], [live({ detail: "thinking", at: 1234 })]);
    expect(one[0].id).toBe("live:ada:1234");
    expect(again[0].id).toBe(one[0].id);
    expect(one[0].live).toBe(true);
  });

  it("is empty for an agent with nothing recorded and nothing in flight", () => {
    expect(mergeLog([], [])).toEqual([]);
  });
});

describe("logText", () => {
  it("reads as tool then detail", () => {
    const [line] = mergeLog(
      [entry({ id: "a", persona_id: "ada", tool: "run_command", detail: "cargo test" })],
      [],
    );
    expect(logText(line)).toBe("run_command cargo test");
  });

  it("falls back to the phase when there is nothing else to say", () => {
    const [line] = mergeLog([entry({ id: "a", persona_id: "ada", status: "thinking" })], []);
    expect(logText(line)).toBe("thinking");
  });

  it("says how many times a collapsed line happened", () => {
    const [line] = mergeLog(
      [
        entry({ id: "a", persona_id: "ada", detail: "x", created_at: "2026-09-01 16:00:00" }),
        entry({ id: "b", persona_id: "ada", detail: "x", created_at: "2026-09-01 16:00:10" }),
      ],
      [],
    );
    expect(logText(line)).toBe("x (x2)");
  });
});
