import { useEffect, useRef } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { WorkspacePanel } from "./components/WorkspacePanel";
import { AnalyticsPage } from "./components/agent/AnalyticsPage";
import { ServerPanel } from "./components/ServerPanel";
import { CanvasFlow } from "./components/CanvasFlow";
import { BottomBar } from "./components/BottomBar";
import { SettingsModal } from "./components/settings/SettingsModal";
import { DialogHost } from "./components/Dialog";
import { TooltipHost } from "./components/Tooltip";
import { AppToolbar } from "./components/AppToolbar";
import { NavRail } from "./components/NavRail";
import { StatusStrip } from "./components/StatusStrip";
import { ChangesPanel } from "./components/agent/ChangesPanel";
import { PlanModal } from "./components/agent/PlanModal";
import { UpdateNotice } from "./components/UpdateNotice";
import { TransfersPanel } from "./components/TransfersPanel";
import { PluginMarketplaceModal } from "./components/plugins/PluginMarketplaceModal";
import { QuickOpenPalette } from "./components/QuickOpenPalette";
import { usePluginStore } from "./stores/pluginStore";
import { useUpdateStore } from "./stores/updateStore";
import { useCanvasStore } from "./stores/canvasStore";
import { useAgentStore } from "./stores/agentStore";
import { useEditsStore } from "./stores/editsStore";
import { useUiStore } from "./stores/uiStore";
import { useThemeStore } from "./stores/themeStore";
import { useSettingsStore } from "./stores/settingsStore";
import { useAgentStatusStore } from "./stores/agentStatusStore";
import { onAgentWorkspaceStatus, onFileChange, onFileChangeReverted, onVpsUpdated } from "./lib/tauri";
import { useVpsStore } from "./stores/vpsStore";
import { useLockStore } from "./stores/lockStore";
import { useAutoLock } from "./hooks/useAutoLock";
import { useOsFileDrop } from "./hooks/useOsFileDrop";
import { useWorkspaceAutosave } from "./hooks/useWorkspaceAutosave";
import { DragGhost } from "./components/DragGhost";
import { SplashScreen, UnlockScreen } from "./components/lock/UnlockScreen";
import { useTileShortcuts } from "./hooks/useTileShortcuts";
import {
  DRAWER_WIDTH_MAX,
  DRAWER_WIDTH_MIN,
  drawerWidthFromDrag,
  drawerWidthFromKey,
  type DrawerSide,
} from "./lib/uiLayout";

function DrawerSplitter({
  side,
  width,
  onWidthChange,
}: {
  side: DrawerSide;
  width: number;
  onWidthChange: (width: number) => void;
}) {
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);
  const previousUserSelect = useRef("");

  const stopDragging = (event: React.PointerEvent<HTMLDivElement>) => {
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    document.body.style.userSelect = previousUserSelect.current;
  };

  return (
    <div
      className="xc-splitter"
      data-side={side}
      role="separator"
      aria-label={side === "left" ? "Resize workspace drawer" : "Resize server drawer"}
      aria-orientation="vertical"
      aria-valuemin={DRAWER_WIDTH_MIN}
      aria-valuemax={DRAWER_WIDTH_MAX}
      aria-valuenow={width}
      tabIndex={0}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        drag.current = { startX: event.clientX, startWidth: width };
        previousUserSelect.current = document.body.style.userSelect;
        document.body.style.userSelect = "none";
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (!drag.current) return;
        onWidthChange(
          drawerWidthFromDrag(
            side,
            drag.current.startWidth,
            drag.current.startX,
            event.clientX,
          ),
        );
      }}
      onPointerUp={stopDragging}
      onPointerCancel={stopDragging}
      onKeyDown={(event) => {
        const next = drawerWidthFromKey(width, event.key);
        if (next === null) return;
        event.preventDefault();
        onWidthChange(next);
      }}
    />
  );
}

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
  const mainView = useUiStore((s) => s.mainView);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const bottomOpen = useUiStore((s) => s.bottomOpen);
  const leftWidth = useUiStore((s) => s.leftWidth);
  const rightWidth = useUiStore((s) => s.rightWidth);
  const setLeftWidth = useUiStore((s) => s.setLeftWidth);
  const setRightWidth = useUiStore((s) => s.setRightWidth);

  const loadTheme = useThemeStore((s) => s.load);
  const agentSessionId = useAgentStore((s) => s.sessionId);
  const subscribeApprovals = useAgentStore((s) => s.subscribeApprovals);
  const pendingApprovalsCount = useAgentStore((s) => s.pendingApprovals.length);
  const pendingQuestionsCount = useAgentStore((s) => s.pendingQuestions.length);
  const hasPendingPlan = useAgentStore((s) => s.pendingPlan !== null);

  const openViews = usePluginStore((s) => s.openViews);
  const definitions = usePluginStore((s) => s.definitions);

  // Alt+arrows / Alt+F / Alt+R reshape the tile grid (see the hook for the full map).
  useTileShortcuts();

  useEffect(() => {
    void loadTheme();
    // Settings used to load only when the agent panel or the settings modal mounted,
    // so anything outside them (the SFTP panel's external-editor entry) read an empty
    // map until the user happened to open one. They're app-wide state — load them once.
    void useSettingsStore.getState().load();
  }, [loadTheme]);

  // Load channel identity, then silently check GitHub for a newer build on that channel.
  // Manual checks live in Settings → General.
  useEffect(() => {
    void useUpdateStore.getState().loadChannel();
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
    let un: (() => void) | undefined;
    onVpsUpdated(() => {
      void useVpsStore.getState().load();
    }).then((u) => {
      un = u;
    });
    return () => un?.();
  }, []);

  useEffect(() => {
    void usePluginStore.getState().loadPlugins();
  }, []);

  useEffect(() => {
    // Surface the agent window whenever it needs the user (approval/question/plan).
    if (pendingApprovalsCount > 0 || pendingQuestionsCount > 0 || hasPendingPlan) {
      useCanvasStore.getState().addAgent();
    }
  }, [pendingApprovalsCount, pendingQuestionsCount, hasPendingPlan]);

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

  return (
    <ReactFlowProvider>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-[var(--bg)]">
        <AppToolbar />

        <div className="flex min-h-0 flex-1 overflow-hidden">
          <NavRail />

          {mainView === "analytics" ? (
            <AnalyticsPage />
          ) : (
            <>
              {leftOpen ? (
                <>
                  <WorkspacePanel width={leftWidth} />
                  <DrawerSplitter
                    side="left"
                    width={leftWidth}
                    onWidthChange={setLeftWidth}
                  />
                </>
              ) : null}

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

              {rightOpen ? (
                <>
                  <DrawerSplitter
                    side="right"
                    width={rightWidth}
                    onWidthChange={setRightWidth}
                  />
                  <ServerPanel width={rightWidth} />
                </>
              ) : null}
            </>
          )}
        </div>

        {mainView === "canvas" && bottomOpen ? <BottomBar /> : null}
        <StatusStrip />
      </div>
      <SettingsModal />
      <PluginMarketplaceModal />
      <QuickOpenPalette />

      {/* Dynamic Plugin Views / Modals (Harness Extension Point) */}
      {Object.entries(openViews).map(([pluginId, isOpen]) => {
        if (!isOpen) return null;
        const def = definitions[pluginId];
        if (!def?.renderView) return null;
        const ViewComp = def.renderView;
        return (
          <div
            key={pluginId}
            className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-6 backdrop-blur-sm"
            onMouseDown={(e) => e.target === e.currentTarget && usePluginStore.getState().closePluginView(pluginId)}
          >
            <div className="h-[85vh] w-[min(1080px,94vw)] overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-2xl flex flex-col animate-in fade-in zoom-in-95 duration-150">
              <ViewComp onClose={() => usePluginStore.getState().closePluginView(pluginId)} />
            </div>
          </div>
        );
      })}

      <PlanModal />
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
