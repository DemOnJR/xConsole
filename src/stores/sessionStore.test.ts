import { beforeEach, describe, expect, it } from "vitest";
import { useSessionStore } from "./sessionStore";

describe("sessionStore", () => {
  beforeEach(() => {
    useSessionStore.getState().clear();
  });

  it("stores and updates session info per node", () => {
    useSessionStore.getState().setInfo("node-1", {
      sessionId: "sid-123",
      status: "connected",
      cwd: "/root/app",
    });

    const info = useSessionStore.getState().sessions["node-1"];
    expect(info).toBeDefined();
    expect(info.sessionId).toBe("sid-123");
    expect(info.status).toBe("connected");
    expect(info.cwd).toBe("/root/app");

    // Partial update
    useSessionStore.getState().setInfo("node-1", {
      cwd: "/root/app/src",
    });

    const updated = useSessionStore.getState().sessions["node-1"];
    expect(updated.sessionId).toBe("sid-123");
    expect(updated.status).toBe("connected");
    expect(updated.cwd).toBe("/root/app/src");
  });

  it("removes a node session info", () => {
    useSessionStore.getState().setInfo("node-1", { sessionId: "sid-123", status: "connected" });
    useSessionStore.getState().setInfo("node-2", { sessionId: "sid-456", status: "connected" });

    useSessionStore.getState().remove("node-1");

    expect(useSessionStore.getState().sessions["node-1"]).toBeUndefined();
    expect(useSessionStore.getState().sessions["node-2"]).toBeDefined();
  });

  it("clears all session info", () => {
    useSessionStore.getState().setInfo("node-1", { sessionId: "sid-123", status: "connected" });
    useSessionStore.getState().setInfo("node-2", { sessionId: "sid-456", status: "connected" });

    useSessionStore.getState().clear();

    expect(Object.keys(useSessionStore.getState().sessions)).toHaveLength(0);
  });
});
