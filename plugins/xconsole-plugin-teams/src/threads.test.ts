import { describe, expect, it } from "vitest";
import type { AgentLogEntry, AgentMessage } from "../../../src/lib/tauri";
import {
  buildIndex,
  parentSummary,
  repliesTo,
  replyCounts,
  resolveParent,
  threadRoots,
} from "./threads";

const msg = (partial: Partial<AgentMessage> & Pick<AgentMessage, "id" | "body">): AgentMessage => ({
  kind: "note",
  ...partial,
});

const entry = (
  partial: Partial<AgentLogEntry> & Pick<AgentLogEntry, "id" | "persona_id">,
): AgentLogEntry => ({
  session_id: "",
  status: "working",
  detail: "",
  ...partial,
});

describe("resolveParent", () => {
  const index = buildIndex(
    [msg({ id: "m1", body: "deploying" })],
    [entry({ id: "l1", persona_id: "ada", tool: "run_command", detail: "kubectl apply" })],
  );

  it("finds a parent that is a message", () => {
    const got = resolveParent("m1", index);
    expect(got?.kind).toBe("message");
    expect(got?.kind === "message" && got.message.body).toBe("deploying");
  });

  it("finds a parent that is a log line, because a correction attaches to an action", () => {
    const got = resolveParent("l1", index);
    expect(got?.kind).toBe("log");
    expect(got?.kind === "log" && got.entry.tool).toBe("run_command");
  });

  it("is null for an id in neither map, and for no id at all", () => {
    expect(resolveParent("gone", index)).toBeNull();
    expect(resolveParent(null, index)).toBeNull();
    expect(resolveParent(undefined, index)).toBeNull();
  });
});

describe("threadRoots", () => {
  it("keeps top-level messages and hides replies whose parent is present", () => {
    const root = msg({ id: "m1", body: "deploying" });
    const reply = msg({ id: "m2", body: "wrong context", parent_id: "m1" });
    const index = buildIndex([root, reply], []);
    expect(threadRoots([root, reply], index).map((m) => m.id)).toEqual(["m1"]);
  });

  it("shows a reply whose parent is nowhere rather than losing it", () => {
    // The parent is a live log line that was never persisted, or a row pruned since.
    // Hiding the reply would silently delete a correction somebody typed.
    const orphan = msg({ id: "m9", body: "that hit the wrong cluster", parent_id: "live:ada:12" });
    const index = buildIndex([orphan], []);
    expect(threadRoots([orphan], index).map((m) => m.id)).toEqual(["m9"]);
  });

  it("hides a reply once its log parent is loaded", () => {
    const reply = msg({ id: "m9", body: "wrong context", parent_id: "l1" });
    const index = buildIndex([reply], [entry({ id: "l1", persona_id: "ada" })]);
    expect(threadRoots([reply], index)).toEqual([]);
  });
});

describe("repliesTo", () => {
  it("returns one parent's replies oldest first", () => {
    const all = [
      msg({ id: "b", body: "second", parent_id: "m1", created_at: "2026-09-01T16:02:00Z" }),
      msg({ id: "a", body: "first", parent_id: "m1", created_at: "2026-09-01T16:01:00Z" }),
      msg({ id: "c", body: "other thread", parent_id: "m2" }),
      msg({ id: "d", body: "top level" }),
    ];
    expect(repliesTo("m1", all).map((m) => m.body)).toEqual(["first", "second"]);
  });

  it("falls back to a stable order when timestamps are missing", () => {
    const all = [
      msg({ id: "b", body: "b", parent_id: "m1" }),
      msg({ id: "a", body: "a", parent_id: "m1" }),
    ];
    expect(repliesTo("m1", all).map((m) => m.id)).toEqual(["a", "b"]);
  });
});

describe("replyCounts", () => {
  it("counts replies per parent, including parents that are not loaded", () => {
    const counts = replyCounts([
      msg({ id: "1", body: "x", parent_id: "m1" }),
      msg({ id: "2", body: "y", parent_id: "m1" }),
      msg({ id: "3", body: "z", parent_id: "live:ada:12" }),
      msg({ id: "4", body: "top" }),
    ]);
    expect(counts.get("m1")).toBe(2);
    expect(counts.get("live:ada:12")).toBe(1);
    expect(counts.get("4")).toBeUndefined();
  });
});

describe("parentSummary", () => {
  it("flattens a message to one line", () => {
    const index = buildIndex([msg({ id: "m1", body: "line one\n\nline two" })], []);
    expect(parentSummary(resolveParent("m1", index)!)).toBe("line one line two");
  });

  it("reads a log parent as its tool and detail", () => {
    const index = buildIndex(
      [],
      [entry({ id: "l1", persona_id: "ada", tool: "run_command", detail: "kubectl apply" })],
    );
    expect(parentSummary(resolveParent("l1", index)!)).toBe("run_command kubectl apply");
  });

  it("truncates something long enough to break the drawer header", () => {
    const index = buildIndex([msg({ id: "m1", body: "x".repeat(400) })], []);
    const got = parentSummary(resolveParent("m1", index)!);
    expect(got).toHaveLength(140);
    expect(got.endsWith("…")).toBe(true);
  });
});
