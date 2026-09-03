import { describe, expect, it } from "vitest";
import type { StreamEvent } from "../lib/tauri";
import {
  appendTextDelta,
  applyActivityEvent,
  clearAwaitingApproval,
  flattenActivity,
  markAwaitingApproval,
  segmentsFromMessage,
  setActivityClock,
  textFromSegments,
} from "./turnSegments";

const toolStart = (id: string, label: string): StreamEvent => ({
  kind: "Activity",
  data: { type: "ToolStart", data: { id, tool: "run_command", label, detail: "uptime" } },
});

const toolEnd = (id: string): StreamEvent => ({
  kind: "Activity",
  data: { type: "ToolEnd", data: { id, ok: true } },
});

describe("turnSegments", () => {
  it("interleaves text, tools, then more text", () => {
    let segs = appendTextDelta([], "I'll check both hosts.");
    segs = applyActivityEvent(segs, toolStart("c1", "Run on K8S"));
    segs = applyActivityEvent(segs, toolEnd("c1"));
    segs = appendTextDelta(segs, "Both look healthy.");

    expect(segs.map((s) => s.type)).toEqual(["text", "activity", "text"]);
    expect(textFromSegments(segs)).toBe("I'll check both hosts.\n\nBoth look healthy.");
    expect(flattenActivity(segs)).toHaveLength(1);
    expect(flattenActivity(segs)[0].state).toBe("done");
  });

  it("keeps a second tool burst after a mid-turn reply", () => {
    let segs = applyActivityEvent([], toolStart("a", "Run on K8S"));
    segs = applyActivityEvent(segs, toolEnd("a"));
    segs = appendTextDelta(segs, "Need a firewall next.");
    segs = applyActivityEvent(segs, toolStart("b", "Run on PORTAINER"));
    segs = applyActivityEvent(segs, toolEnd("b"));
    segs = appendTextDelta(segs, "Firewall is live.");

    expect(segs.map((s) => s.type)).toEqual(["activity", "text", "activity", "text"]);
    expect(flattenActivity(segs).map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("falls back to text-then-activity when a message has no segments", () => {
    const segs = segmentsFromMessage({
      role: "assistant",
      content: "Done",
      activity: [{ id: "c1", kind: "command", label: "Run on x", state: "done" }],
    });
    expect(segs.map((s) => s.type)).toEqual(["text", "activity"]);
  });
});

const toolCall = (id: string, name: string, args: unknown): StreamEvent => ({
  kind: "ToolCall",
  data: { id, name, arguments: args },
});

const toolResult = (id: string, output: string): StreamEvent => ({
  kind: "ToolResult",
  data: { id, output },
});

describe("tool call arguments", () => {
  it("keeps what the model actually asked for, not just the tool name", () => {
    // The backend has always sent the whole call; the UI kept `name` and threw the rest
    // away, so a running tool read "read file" with no indication of which file.
    const segs = applyActivityEvent([], toolCall("t1", "read_file", { path: "/etc/nginx.conf" }));
    const [item] = flattenActivity(segs);
    expect(item.arguments).toEqual({ path: "/etc/nginx.conf" });
    expect(item.tool).toBe("read_file");
  });
});

describe("tool outcomes", () => {
  it("believes the exit code over the shape of the output text", () => {
    // `startsWith("error")` was wrong in both directions. A command whose own output
    // opens with the word "error" is not a failed tool call...
    let segs = applyActivityEvent([], toolCall("t1", "run_command", { command: "grep x log" }));
    segs = applyActivityEvent(segs, toolResult("t1", "exit_code: 0\nstdout:\nerror rate: 0.1%"));
    expect(flattenActivity(segs)[0].state).toBe("done");
    expect(flattenActivity(segs)[0].exitCode).toBe(0);

    // ...and a command that exits non-zero with a polite message is not a success.
    let other = applyActivityEvent([], toolCall("t2", "run_command", { command: "systemctl start x" }));
    other = applyActivityEvent(other, toolResult("t2", "exit_code: 1\nstderr:\nUnit not found"));
    expect(flattenActivity(other)[0].state).toBe("error");
    expect(flattenActivity(other)[0].exitCode).toBe(1);
  });

  it("still falls back to an error prefix when there is no exit code", () => {
    let segs = applyActivityEvent([], toolCall("t1", "read_file", { path: "/nope" }));
    segs = applyActivityEvent(segs, toolResult("t1", "error: missing path"));
    expect(flattenActivity(segs)[0].state).toBe("error");
    // Anchored, so prose that merely mentions an error is not a failure.
    let ok = applyActivityEvent([], toolCall("t2", "read_file", { path: "/x" }));
    ok = applyActivityEvent(ok, toolResult("t2", "the errors are logged to /var/log"));
    expect(flattenActivity(ok)[0].state).toBe("done");
  });

  it("records that output was cut off, because the agent did not see the rest either", () => {
    let segs = applyActivityEvent([], toolCall("t1", "run_command", { command: "cat big.log" }));
    segs = applyActivityEvent(segs, toolResult("t1", "exit_code: 0\nlots\n\n[Output truncated: 900 lines omitted]"));
    expect(flattenActivity(segs)[0].truncated).toBe(true);
  });

  it("measures how long a tool took", () => {
    let t = 1_000;
    const restore = setActivityClock(() => t);
    try {
      let segs = applyActivityEvent([], toolCall("t1", "run_command", { command: "sleep 2" }));
      t = 3_400;
      segs = applyActivityEvent(segs, toolResult("t1", "exit_code: 0"));
      expect(flattenActivity(segs)[0].durationMs).toBe(2_400);
    } finally {
      setActivityClock(restore);
    }
  });
});

describe("awaiting approval", () => {
  it("distinguishes a tool waiting for a person from one that is working", () => {
    // A command held at the safety gate emits nothing further, so it kept the running
    // spinner forever and a blocked turn looked like a busy one.
    let segs = applyActivityEvent([], toolCall("t1", "run_command", { command: "rm -rf /srv/app" }));
    expect(flattenActivity(segs)[0].state).toBe("running");

    segs = markAwaitingApproval(segs, "rm -rf /srv/app");
    expect(flattenActivity(segs)[0].state).toBe("awaiting_approval");

    segs = clearAwaitingApproval(segs);
    expect(flattenActivity(segs)[0].state).toBe("running");
  });

  it("leaves tools alone when the approval is for something else", () => {
    let segs = applyActivityEvent([], toolCall("t1", "run_command", { command: "uptime" }));
    segs = markAwaitingApproval(segs, "rm -rf /srv/app");
    expect(flattenActivity(segs)[0].state).toBe("running");
  });
});
