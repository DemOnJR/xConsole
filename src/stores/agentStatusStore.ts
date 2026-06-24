import { create } from "zustand";

// Tracks what the agent is doing per workspace (working / planning / testing /
// idle), shown as a dot + hover label on the workspace row. Fed by the
// "agent://workspace-status" event (see App.tsx subscription).

export type AgentStatus = "working" | "planning" | "testing" | "idle";

interface AgentStatusState {
  /** workspace_id → status. Idle/unknown workspaces are absent. */
  byWorkspace: Record<string, AgentStatus>;
  set: (workspaceId: string, status: string) => void;
}

export const useAgentStatusStore = create<AgentStatusState>((set) => ({
  byWorkspace: {},
  set: (workspaceId, status) =>
    set((s) => {
      const next = { ...s.byWorkspace };
      if (status === "idle") {
        delete next[workspaceId];
      } else {
        next[workspaceId] = status as AgentStatus;
      }
      return { byWorkspace: next };
    }),
}));

/** Display label + dot color for a status. */
export const STATUS_META: Record<AgentStatus, { label: string; color: string }> = {
  working: { label: "Working…", color: "#3b82f6" },
  planning: { label: "Planning…", color: "#a855f7" },
  testing: { label: "Testing…", color: "#22c55e" },
  idle: { label: "Idle", color: "#6b7280" },
};
