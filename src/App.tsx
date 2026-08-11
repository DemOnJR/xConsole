import { useEffect } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { WorkspacePanel } from "./components/WorkspacePanel";
import { ServerPanel } from "./components/ServerPanel";
import { CanvasFlow } from "./components/CanvasFlow";
import { BottomBar } from "./components/BottomBar";
import { SettingsModal } from "./components/settings/SettingsModal";
import { DialogHost } from "./components/Dialog";
import { TooltipHost } from "./components/Tooltip";
import { AgentPanel } from "./components/agent/AgentPanel";
import { AppToolbar } from "./components/AppToolbar";
import { NavRail } from "./components/NavRail";
import { StatusStrip } from "./components/StatusStrip";
import { ChangesPanel } from "./components/agent/ChangesPanel";
import { UpdateNotice } from "./components/UpdateNotice";
import { TransfersPanel } from "./components/TransfersPanel";
import { useUpdateStore } from "./stores/updateStore";
import { useCanvasStore } from "./stores/canvasStore";
import { useAgentStore } from "./stores/agentStore";
import { useEditsStore } from "./stores/editsStore";
import { useUiStore } from "./stores/uiStore";
import { useThemeStore } from "./stores/themeStore";
import { useSettingsStore } from "./stores/settingsStore";
import { useAgentStatusStore } from "./stores/agentStatusStore";
import { onAgentWorkspaceStatus, onFileChange, onFileChangeReverted } from "./lib/tauri";
import { useLockStore } from "./stores/lockStore";
import { useAutoLock } from "./hooks/useAutoLock";
import { useOsFileDrop } from "./hooks/useOsFileDrop";
import { useWorkspaceAutosave } from "./hooks/useWorkspaceAutosave";
import { DragGhost } from "./components/DragGhost";
import { SplashScreen, UnlockScreen } from "./components/lock/UnlockScreen";
import { useTileShortcuts } from "./hooks/useTileShortcuts";

/** Restores the workspace as it was left and keeps saving it. A component rather than a
 *  hook call in UnlockedApp because it needs React Flow's viewport, and UnlockedApp's own
 *  body sits outside the <ReactFlowProvider> it renders. */
function WorkspaceAutosave() {
  useWorkspaceAutosave();
  return null;
}

// The real app body. Only mounts once unlocked, so none of its DB-touching effects
// (theme load, agent/edits subscriptions) run while the database is still encrypted/locked.
function UnlockedApp() {
  const nodes = useCanvasStore((s) => s.nodes);
  const focus = useCanvasStore((s) => s.focus);
  // One window-level listener for files dragged in from Explorer; each drop target
  // filters by its own id.
  useOsFileDrop();

  const leftOpen = useUiStore((s) => s.leftOpen);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const bottomOpen = useUiStore((s) => s.bottomOpen);
  const agentOpen = useUiStore((s) => s.agentOpen);
  const agentExpanded = useUiStore((s) => s.agentExpanded);
  const setAgentOpen = useUiStore((s) => s.setAgentOpen);

  const loadTheme = useThemeStore((s) => s.load);
  const agentSessionId = useAgentStore((s) => s.sessionId);
  const subscribeApprovals = useAgentStore((s) => s.subscribeApprovals);
  const pendingApprovalsCount = useAgentStore((s) => s.pendingApprovals.length);
  const pendingQuestionsCount = useAgentStore((s) => s.pendingQuestions.length);
  const hasPendingPlan = useAgentStore((s) => s.pendingPlan !== null);

  // Alt+arrows / Alt+F / Alt+R reshape the tile grid (see the hook for the full map).
  useTileShortcuts();

  useEffect(() => {
    void loadTheme();
    // Settings used to load only when the agent panel or the settings modal mounted,
    // so anything outside them (the SFTP panel's external-editor entry) read an empty
    // map until the user happened to open one. They're app-wide state — load them once.
    void useSettingsStore.getState().load();
  }, [loadTheme]);

  // Check GitHub for a newer signed release shortly after launch (silent — only
  // shows a card if one is available). Manual checks live in Settings → General.
  useEffect(() => {
    const t = setTimeout(() => void useUpdateStore.getState().check(false), 4000);
    return () => clearTimeout(t);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const setStatus = useAgentStatusStore.getState().set;
    onAgentWorkspaceStatus((s) => setStatus(s.workspace_id, s.status)).then(
      (u) => (unlisten = u),
    );
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    subscribeApprovals().then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [subscribeApprovals]);

  // Load the agent's recorded file edits for the active chat session, and keep the
  // changes panel updated live as the agent writes/reverts files.
  useEffect(() => {
    void useEditsStore.getState().sync(agentSessionId ?? null);
  }, [agentSessionId]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    onFileChange((c) => useEditsStore.getState().ingest(c)).then((u) => unlisteners.push(u));
    onFileChangeReverted((id) => useEditsStore.getState().markReverted(id)).then((u) =>
      unlisteners.push(u),
    );
    return () => unlisteners.forEach((u) => u());
  }, []);

  useEffect(() => {
    // Surface the agent panel whenever it needs the user (approval/question/plan).
    if (pendingApprovalsCount > 0 || pendingQuestionsCount > 0 || hasPendingPlan) {
      setAgentOpen(true);
    }
  }, [pendingApprovalsCount, pendingQuestionsCount, hasPendingPlan, setAgentOpen]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        const list = useCanvasStore.getState().nodes;
        if (list.length === 0) return;
        const cur = useCanvasStore.getState().focusedId;
        const idx = list.findIndex((n) => n.id === cur);
        const next = list[(idx + 1) % list.length];
        focus(next.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focus]);

  const agentOnly = agentOpen && agentExpanded;

  return (
    <ReactFlowProvider>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-[var(--bg)]">
        <AppToolbar />

        <div className="flex min-h-0 flex-1 overflow-hidden">
          <NavRail />

          {agentOnly ? (
            <AgentPanel expanded />
          ) : (
            <>
              {leftOpen ? <WorkspacePanel /> : null}

              <main className="relative min-w-0 flex-1">
                <CanvasFlow />
                {nodes.length === 0 && (
                  <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
                    <div className="max-w-sm px-6 text-center">
                      <p className="text-base text-[var(--text-dim)]">
                        Drag a server from the hosts panel onto the canvas, or click it.
                      </p>
                      <p className="mt-2 text-sm text-[var(--text-faint)]">
                        Use the left rail for workspaces, servers, agent, and console.
                      </p>
                    </div>
                  </div>
                )}
              </main>

              {rightOpen ? <ServerPanel /> : null}
              {agentOpen ? <AgentPanel /> : null}
            </>
          )}
        </div>

        {bottomOpen && !agentOnly ? <BottomBar /> : null}
        <StatusStrip />
      </div>
      <SettingsModal />
      <ChangesPanel />
      <TransfersPanel />
      <UpdateNotice />
      <DialogHost />
      <TooltipHost />
      <DragGhost />
      <WorkspaceAutosave />
    </ReactFlowProvider>
  );
}

// Gate: hold the whole app behind the unlock screen while the DB is locked.
export default function App() {
  const status = useLockStore((s) => s.status);
  // Mounted on the gate, not inside UnlockedApp, so the `app://locked` listener is alive
  // in every state — including while the unlock screen is up, so a second window or the
  // idle timer can't leave this one showing a stale unlocked UI.
  useAutoLock();
  useEffect(() => {
    void useLockStore.getState().check();
  }, []);
  if (status === "loading") return <SplashScreen />;
  if (status === "locked") return <UnlockScreen />;
  return <UnlockedApp />;
}
