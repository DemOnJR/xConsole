import { Panel, useReactFlow } from "@xyflow/react";
import { useCanvasStore, type LayoutMode } from "../stores/canvasStore";
import { useSessionStore } from "../stores/sessionStore";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { api } from "../lib/tauri";
import {
  EraserIcon,
  GridIcon,
  MaximizeIcon,
  SaveAsIcon,
  SaveIcon,
} from "./icons";

const MODES: { id: LayoutMode; label: string; title: string }[] = [
  { id: "freeform", label: "Freeform", title: "Drag terminals anywhere" },
  { id: "snap", label: "Snap", title: "Snap to grid while dragging" },
  { id: "tile", label: "Tile", title: "Auto-arrange into a grid" },
];

const ICON_BTN =
  "flex items-center justify-center rounded-md border border-[#1f2737] p-1.5 text-gray-300 hover:bg-[#1f2737] hover:text-white";

export function Toolbar() {
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const setLayout = useCanvasStore((s) => s.setLayout);
  const arrangeTiles = useCanvasStore((s) => s.arrangeTiles);
  const clear = useCanvasStore((s) => s.clear);
  const nodes = useCanvasStore((s) => s.nodes);
  const sessions = useSessionStore((s) => s.sessions);
  const { fitView, getViewport } = useReactFlow();

  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const activeId = useWorkspaceStore((s) => s.activeId);
  const saveWorkspace = useWorkspaceStore((s) => s.save);
  const activeWorkspace = workspaces.find((w) => w.id === activeId);

  const updateCurrent = async () => {
    if (activeWorkspace) {
      await saveWorkspace(
        activeWorkspace.name,
        getViewport(),
        activeWorkspace.id,
      );
    } else {
      await saveAsNew();
    }
  };

  const saveAsNew = async () => {
    const name = window.prompt("Workspace name");
    if (!name) return;
    await saveWorkspace(name, getViewport());
  };

  const clearCanvas = () => {
    // Clear is an explicit teardown, so kill the live sessions (unlike a
    // workspace switch, which keeps them alive in the background).
    const removeInfo = useSessionStore.getState().remove;
    nodes.forEach((n) => {
      const sid = sessions[n.id]?.sessionId;
      if (sid) api.sshDisconnect(sid).catch(() => {});
      removeInfo(n.id);
    });
    clear();
  };

  return (
    <Panel position="top-left" className="!m-2">
      <div className="flex items-center gap-2 rounded-lg border border-[#1f2737] bg-[#11161f]/95 px-2 py-1.5 shadow-lg backdrop-blur">
        {/* Layout modes */}
        <div className="flex overflow-hidden rounded-md border border-[#1f2737]">
          {MODES.map((m) => (
            <button
              key={m.id}
              title={m.title}
              onClick={() => setLayout(m.id)}
              className={`px-2.5 py-1 text-xs ${
                layoutMode === m.id
                  ? "bg-blue-600 text-white"
                  : "bg-transparent text-gray-300 hover:bg-[#1f2737]"
              }`}
            >
              {m.label}
            </button>
          ))}
        </div>

        <button title="Re-tile now" onClick={() => arrangeTiles()} className={ICON_BTN}>
          <GridIcon size={15} />
        </button>
        <button
          title="Fit all terminals in view"
          onClick={() => fitView({ duration: 300, padding: 0.15 })}
          className={ICON_BTN}
        >
          <MaximizeIcon size={15} />
        </button>

        <div className="mx-0.5 h-5 w-px bg-[#1f2737]" />

        {/* Workspace actions */}
        <button
          title={
            activeWorkspace
              ? `Update workspace "${activeWorkspace.name}"`
              : "Save canvas as a workspace"
          }
          onClick={updateCurrent}
          className="flex items-center gap-1 rounded-md bg-blue-600 px-2 py-1 text-xs text-white hover:bg-blue-500"
        >
          <SaveIcon size={15} />
          {activeWorkspace ? "Update" : "Save"}
        </button>
        <button title="Save as a new workspace" onClick={saveAsNew} className={ICON_BTN}>
          <SaveAsIcon size={15} />
        </button>
        <button title="Clear canvas" onClick={() => clearCanvas()} className={ICON_BTN}>
          <EraserIcon size={15} />
        </button>
      </div>
    </Panel>
  );
}
