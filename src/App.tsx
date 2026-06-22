import { useEffect } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { WorkspacePanel } from "./components/WorkspacePanel";
import { ServerPanel } from "./components/ServerPanel";
import { CanvasFlow } from "./components/CanvasFlow";
import { BottomBar } from "./components/BottomBar";
import { SettingsModal } from "./components/settings/SettingsModal";
import { AgentPanel } from "./components/agent/AgentPanel";
import { AppToolbar } from "./components/AppToolbar";
import { useCanvasStore } from "./stores/canvasStore";
import { useAgentStore } from "./stores/agentStore";
import { useUiStore } from "./stores/uiStore";

export default function App() {
  const nodes = useCanvasStore((s) => s.nodes);
  const focus = useCanvasStore((s) => s.focus);

  const leftOpen = useUiStore((s) => s.leftOpen);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const bottomOpen = useUiStore((s) => s.bottomOpen);
  const agentOpen = useUiStore((s) => s.agentOpen);
  const agentExpanded = useUiStore((s) => s.agentExpanded);
  const setAgentOpen = useUiStore((s) => s.setAgentOpen);

  const subscribeApprovals = useAgentStore((s) => s.subscribeApprovals);
  const pendingApprovalsCount = useAgentStore((s) => s.pendingApprovals.length);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    subscribeApprovals().then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [subscribeApprovals]);

  useEffect(() => {
    if (pendingApprovalsCount > 0) setAgentOpen(true);
  }, [pendingApprovalsCount, setAgentOpen]);

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
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-[#0b0f17]">
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
    </ReactFlowProvider>
  );
}
