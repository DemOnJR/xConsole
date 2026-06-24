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
import { ChangesPanel } from "./components/agent/ChangesPanel";
import { UpdateNotice } from "./components/UpdateNotice";
import { useUpdateStore } from "./stores/updateStore";
import { useCanvasStore } from "./stores/canvasStore";
import { useAgentStore } from "./stores/agentStore";
import { useEditsStore } from "./stores/editsStore";
import { useUiStore } from "./stores/uiStore";
import { useThemeStore } from "./stores/themeStore";
import { useAgentStatusStore } from "./stores/agentStatusStore";
import { onAgentWorkspaceStatus, onFileChange, onFileChangeReverted } from "./lib/tauri";

export default function App() {
  const nodes = useCanvasStore((s) => s.nodes);
  const focus = useCanvasStore((s) => s.focus);

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

  useEffect(() => {
    void loadTheme();
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
          {agentOnly ? (
            <AgentPanel expanded />
          ) : (
            <>
              {leftOpen ? <WorkspacePanel /> : null}

              <main className="relative min-w-0 flex-1">
                <CanvasFlow />
                {nodes.length === 0 && (
                  <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
                    <div className="text-center">
                      <p className="text-lg text-gray-500">
                        Drag a server from the right onto the canvas (or click it).
                      </p>
                      <p className="mt-1 text-sm text-gray-600">
                        Zoom and pan to watch all your VPS at once.
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
      </div>
      <SettingsModal />
      <ChangesPanel />
      <UpdateNotice />
      <DialogHost />
      <TooltipHost />
    </ReactFlowProvider>
  );
}
