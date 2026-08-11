import { useEffect, useMemo } from "react";
import { useSessionStore } from "../stores/sessionStore";
import { useCanvasStore } from "../stores/canvasStore";
import { useAgentStore } from "../stores/agentStore";
import { useTransferStore } from "../stores/transferStore";
import { useVpsStore } from "../stores/vpsStore";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useUiStore } from "../stores/uiStore";
import { useUpdateStore } from "../stores/updateStore";

/**
 * Compact bottom status strip — always visible, minimal.
 * Surfaces connection health, agent state, and transfers without another panel.
 */
export function StatusStrip() {
  const sessions = useSessionStore((s) => s.sessions);
  const nodes = useCanvasStore((s) => s.nodes);
  const streaming = useAgentStore((s) => s.streaming);
  const streamStats = useAgentStore((s) => s.streamStats);
  const pendingApprovals = useAgentStore((s) => s.pendingApprovals.length);
  const pendingQuestions = useAgentStore((s) => s.pendingQuestions.length);
  const hasPlan = useAgentStore((s) => s.pendingPlan !== null);
  const jobs = useTransferStore((s) => s.jobs);
  const vpsCount = useVpsStore((s) => s.vpsList.length);
  const activeWs = useWorkspaceStore((s) => s.activeId);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const setAgentOpen = useUiStore((s) => s.setAgentOpen);
  const leftOpen = useUiStore((s) => s.leftOpen);
  const toggleLeft = useUiStore((s) => s.toggleLeft);
  const setTransfersOpen = useTransferStore((s) => s.setOpen);
  const openSettings = useUiStore((s) => s.openSettings);
  const channel = useUpdateStore((s) => s.channel);
  const currentSha = useUpdateStore((s) => s.current);
  const loadChannel = useUpdateStore((s) => s.loadChannel);

  useEffect(() => {
    void loadChannel();
  }, [loadChannel]);

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

  /** Aggregate transfer progress when jobs report bytes. */
  const transferProgress = useMemo(() => {
    const active = Object.values(jobs).filter(
      (j) => j.state === "running" || j.state === "scanning",
    );
    if (active.length === 0) return null;
    let done = 0;
    let total = 0;
    for (const j of active) {
      if (j.bytes_total > 0) {
        total += j.bytes_total;
        done += Math.min(j.bytes_done, j.bytes_total);
      }
    }
    if (total <= 0) return null;
    return Math.round((done / total) * 100);
  }, [jobs]);

  /** Git branch of the focused canvas node, if any. */
  const focusedId = useCanvasStore((s) => s.focusedId);
  const focusGit = useMemo(() => {
    if (!focusedId) return null;
    const info = sessions[focusedId];
    if (!info?.gitBranch) return null;
    return { branch: info.gitBranch, dirty: Boolean(info.gitDirty) };
  }, [focusedId, sessions]);

  const activity = useAgentStore((s) => s.activity);
  const runningTools = activity.filter((a) => a.state === "running").length;

  const tokRate =
    streamStats && streamStats.tokensPerSec > 0
      ? ` · ${streamStats.tokensPerSec.toFixed(1)} t/s`
      : "";
  const agentLabel = streaming
    ? runningTools > 1
      ? `Agent · ${runningTools} tools…${tokRate}`
      : `Agent working…${tokRate}`
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
      <button
        type="button"
        className="truncate text-[var(--text-dim)] transition hover:text-[var(--text)]"
        title={`${wsName} — open workspaces`}
        onClick={() => {
          if (!leftOpen) toggleLeft();
        }}
      >
        {wsName}
      </button>

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

      {focusGit ? (
        <>
          <span className="text-[var(--border-strong)]">·</span>
          <span
            className="inline-flex max-w-[140px] items-center gap-1 truncate font-mono text-[10px]"
            title={
              focusGit.dirty
                ? `${focusGit.branch} (dirty)`
                : focusGit.branch
            }
          >
            <span className="text-[var(--text-faint)]">⎇</span>
            <span className="truncate text-[var(--text-dim)]">{focusGit.branch}</span>
            {focusGit.dirty ? (
              <span className="text-amber-400" title="Uncommitted changes">
                *
              </span>
            ) : null}
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
            {transferProgress != null ? ` · ${transferProgress}%` : ""}
          </button>
        ) : null}

        <button
          type="button"
          className="inline-flex items-center gap-1.5 rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide transition hover:bg-[var(--surface-hover)]"
          title="Release channel — open Settings → General to switch"
          onClick={() => openSettings("general")}
        >
          <span
            className={
              channel === "dev" ? "text-amber-300" : "text-[var(--text-faint)]"
            }
          >
            {channel === "dev" ? "dev" : "stable"}
          </span>
          {currentSha ? (
            <span className="font-mono normal-case tracking-normal text-[var(--text-faint)]">
              {currentSha}
            </span>
          ) : null}
        </button>

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
