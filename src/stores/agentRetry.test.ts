import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAgentStore } from "./agentStore";

describe("agentStore retryLast & error recovery", () => {
  beforeEach(() => {
    useAgentStore.setState({
      messages: [],
      streaming: false,
      error: null,
    });
  });

  it("does not wipe history when assistant has tool activity (research/plan) and resumes with continuation", async () => {
    const sendSpy = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({
      send: sendSpy,
      error: "429 Rate limit exceeded",
      messages: [
        { role: "user", content: "Set up nginx and configure SSL" },
        {
          role: "assistant",
          content: "",
          activity: [
            {
              id: "a1",
              kind: "tool",
              label: "Investigate server",
              tool: "run_command",
              state: "done",
            },
            {
              id: "a2",
              kind: "tool",
              label: "Present Plan",
              tool: "present_plan",
              state: "done",
            },
          ],
        },
      ],
    });

    await useAgentStore.getState().retryLast();

    // History must NOT be wiped!
    const msgs = useAgentStore.getState().messages;
    expect(msgs).toHaveLength(2);
    expect(msgs[0].content).toBe("Set up nginx and configure SSL");
    expect(msgs[1].activity).toHaveLength(2);

    // Error must be cleared and continuation prompt must be sent
    expect(useAgentStore.getState().error).toBeNull();
    expect(sendSpy).toHaveBeenCalledTimes(1);
    expect(sendSpy.mock.calls[0][0]).toContain("429 Rate limit exceeded");
    expect(sendSpy.mock.calls[0][0]).toContain("continue from where you left off");
  });

  it("does not wipe history when assistant has streamingSegments", async () => {
    const sendSpy = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({
      send: sendSpy,
      error: "Connection timeout",
      messages: [
        { role: "user", content: "Migrate database" },
        {
          role: "assistant",
          content: "",
          segments: [
            {
              type: "activity",
              items: [
                {
                  id: "s1",
                  kind: "tool",
                  label: "Read schema",
                  tool: "read_file",
                  state: "done",
                },
              ],
            },
          ],
        },
      ],
    });

    await useAgentStore.getState().retryLast();

    const msgs = useAgentStore.getState().messages;
    expect(msgs).toHaveLength(2);
    expect(sendSpy).toHaveBeenCalledTimes(1);
    expect(sendSpy.mock.calls[0][0]).toContain("Connection timeout");
    expect(sendSpy.mock.calls[0][0]).toContain("continue");
  });

  it("safely re-sends user turn from scratch when failure occurred immediately with no assistant work", async () => {
    const sendSpy = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({
      send: sendSpy,
      error: "401 Unauthorized",
      messages: [
        { role: "user", content: "First turn" },
        { role: "assistant", content: "First turn done" },
        { role: "user", content: "Second turn that failed immediately" },
      ],
    });

    await useAgentStore.getState().retryLast();

    // The failed turn was removed so send() will re-append it cleanly
    const msgs = useAgentStore.getState().messages;
    expect(msgs).toHaveLength(2);
    expect(msgs[0].content).toBe("First turn");
    expect(msgs[1].content).toBe("First turn done");

    expect(useAgentStore.getState().error).toBeNull();
    expect(sendSpy).toHaveBeenCalledWith("Second turn that failed immediately", {
      images: undefined,
    });
  });
});
