import { useMemo } from "react";
import { useSessionStore } from "../stores/sessionStore";
import { useCanvasStore } from "../stores/canvasStore";
import { useAgentStore } from "../stores/agentStore";
import { useTransferStore } from "../stores/transferStore";
import { useVpsStore } from "../stores/vpsStore";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useUiStore } from "../stores/uiStore";

/**
 * Compact bottom status strip — always visible, minimal.
 * Surfaces connection health, agent state, and transfers without another panel.
 */
export function StatusStrip() {
  const sessions = useSessionStore((s) => s.sessions);
  const nodes = useCanvasStore((s) => s.nodes);
  const streaming = useAgentStore((s) => s.streaming);
  const pendingApprovals = useAgentStore((s) => s.pendingApprovals.length);
  const pendingQuestions = useAgentStore((s) => s.pendingQuestions.length);
  const hasPlan = useAgentStore((s) => s.pendingPlan !== null);
  const jobs = useTransferStore((s) => s.jobs);
  const vpsCount = useVpsStore((s) => s.vpsList.length);
  const activeWs = useWorkspaceStore((s) => s.activeId);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const setAgentOpen = useUiStore((s) => s.setAgentOpen);
  const setTransfersOpen = useTransferStore((s) => s.setOpen);

  const wsName = useMemo(() => {
    if (!activeWs) return "No workspace";
    return workspaces.find((w) => w.id === activeWs)?.name ?? "Workspace";
  }, [activeWs, workspaces]);

  const conn = useMemo(() => {
    let connected = 0;
    let error = 0;
    let connecting = 0;
    for (const s of Object.values(sessions)) {
      if (s.status === "connected") connected += 1;
      else if (s.status === "error") error += 1;
      else if (s.status === "connecting" || s.status === "reconnecting") connecting += 1;
    }
    return { connected, error, connecting, total: Object.keys(sessions).length };
  }, [sessions]);

  const activeTransfers = useMemo(
    () =>
      Object.values(jobs).filter((j) => j.state === "running" || j.state === "scanning")
        .length,
    [jobs],
  );

  const agentLabel = streaming
    ? "Agent working…"
    : hasPlan
      ? "Plan awaiting approval"
      : pendingApprovals > 0
        ? `Approval needed (${pendingApprovals})`
        : pendingQuestions > 0
          ? `Question pending (${pendingQuestions})`
          : "Agent idle";

  const agentTone = streaming
    ? "var(--accent)"
    : hasPlan || pendingApprovals > 0 || pendingQuestions > 0
      ? "var(--warning)"
      : "var(--text-faint)";

  return (
    <footer className="xc-status-strip" role="status">
      <span className="truncate text-[var(--text-dim)]" title={wsName}>
        {wsName}
      </span>

      <span className="text-[var(--border-strong)]">·</span>

      <span title={`${vpsCount} saved servers`}>
        {vpsCount} server{vpsCount === 1 ? "" : "s"}
      </span>

      <span className="text-[var(--border-strong)]">·</span>

      <span title={`${nodes.length} panels on canvas`}>
        {nodes.length} panel{nodes.length === 1 ? "" : "s"}
      </span>

      {conn.total > 0 ? (
        <>
          <span className="text-[var(--border-strong)]">·</span>
          <span className="inline-flex items-center gap-1.5">
            <span
              className="inline-block h-1.5 w-1.5 rounded-full"
              style={{
                background:
                  conn.error > 0
                    ? "var(--danger)"
                    : conn.connecting > 0
                      ? "var(--warning)"
                      : "var(--success)",
              }}
            />
            {conn.connected} live
            {conn.connecting > 0 ? ` · ${conn.connecting} …` : ""}
            {conn.error > 0 ? ` · ${conn.error} err` : ""}
          </span>
        </>
      ) : null}

      <div className="ml-auto flex items-center gap-3">
        {activeTransfers > 0 ? (
          <button
            type="button"
            className="text-[var(--text-dim)] transition hover:text-[var(--text)]"
            onClick={() => setTransfersOpen(true)}
          >
            {activeTransfers} transfer{activeTransfers === 1 ? "" : "s"}
          </button>
        ) : null}

        <button
          type="button"
          className="inline-flex items-center gap-1.5 transition hover:opacity-90"
          style={{ color: agentTone }}
          onClick={() => setAgentOpen(true)}
        >
          <span
            className="inline-block h-1.5 w-1.5 rounded-full"
            style={{ background: agentTone }}
          />
          {agentLabel}
        </button>
      </div>
    </footer>
  );
}
