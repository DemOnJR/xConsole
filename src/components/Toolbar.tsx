import { useReactFlow, useStore } from "@xyflow/react";
import { useCanvasStore, type LayoutMode } from "../stores/canvasStore";
import { useSessionStore } from "../stores/sessionStore";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { dialog } from "../stores/dialogStore";
import { api } from "../lib/tauri";
import {
  EraserIcon,
  GridIcon,
  MaximizeIcon,
  SaveAsIcon,
  SaveIcon,
} from "./icons";
import { TileLayoutMenu } from "./TileLayoutMenu";

const MODES: { id: LayoutMode; label: string; title: string }[] = [
  { id: "freeform", label: "Freeform", title: "Drag terminals anywhere" },
  {
    id: "tile",
    label: "Tile",
    title: "Snap into a grid, keeping how you've arranged them",
  },
];

const ICON_BTN =
  "flex items-center justify-center rounded-md border border-[var(--border)] p-1.5 text-gray-300 hover:bg-[var(--border)] hover:text-white";

export function Toolbar() {
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const setLayout = useCanvasStore((s) => s.setLayout);
  const retileFromPositions = useCanvasStore((s) => s.retileFromPositions);
  const clear = useCanvasStore((s) => s.clear);
  const nodes = useCanvasStore((s) => s.nodes);
  const sessions = useSessionStore((s) => s.sessions);
  const { fitView, getViewport, setViewport } = useReactFlow();
  const paneW = useStore((s) => s.width);
  const paneH = useStore((s) => s.height);

  // Tile to fill the visible window, taking the arrangement from where the terminals
  // currently sit — drag them roughly into place, press this, and the grid adopts that
  // shape. Then reset zoom to 1 so the terminals render at their normal font size.
  const tileToFit = () => {
    retileFromPositions({ width: paneW, height: paneH });
    setViewport({ x: 0, y: 0, zoom: 1 }, { duration: 300 });
  };

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
    const name = await dialog.prompt({
      title: "Save workspace",
      label: "Name",
      placeholder: "My workspace",
      confirmText: "Save",
    });
    if (!name || !name.trim()) return;
    await saveWorkspace(name.trim(), getViewport());
  };

  const clearCanvas = async () => {
    if (nodes.length === 0) return;
    const ok = await dialog.confirm({
      title: "Clear canvas?",
      message: `Close and disconnect ${nodes.length} panel${nodes.length === 1 ? "" : "s"}? This ends live SSH/SFTP/DB sessions for those panels.`,
      danger: true,
      confirmText: "Clear",
    });
    if (!ok) return;
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
    <div className="flex items-center gap-2">
        {/* Layout modes */}
        <div className="flex overflow-hidden rounded-md border border-[var(--border)]">
          {MODES.map((m) => (
            <button
              key={m.id}
              data-tooltip={m.title}
              onClick={() => {
                setLayout(m.id);
                // Entering tile mode adopts however the user has them arranged now,
                // rather than imposing a fresh grid over their arrangement.
                if (m.id === "tile") tileToFit();
              }}
              className={`px-2.5 py-1 text-xs ${
                layoutMode === m.id
                  ? "bg-blue-600 text-white"
                  : "bg-transparent text-gray-300 hover:bg-[var(--border)]"
              }`}
            >
              {m.label}
            </button>
          ))}
        </div>

        <button
          data-tooltip="Re-tile from where they sit now — drag them first, then press this"
          onClick={tileToFit}
          className={ICON_BTN}
        >
          <GridIcon size={15} />
        </button>
        {layoutMode === "tile" ? <TileLayoutMenu /> : null}
        <button
          data-tooltip="Fit all terminals in view"
          onClick={() => fitView({ duration: 300, padding: 0.15 })}
          className={ICON_BTN}
        >
          <MaximizeIcon size={15} />
        </button>

        <div className="mx-0.5 h-5 w-px bg-[var(--border)]" />

        {/* Workspace actions */}
        <button
          data-tooltip={
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
        <button data-tooltip="Save as a new workspace" onClick={saveAsNew} className={ICON_BTN}>
          <SaveAsIcon size={15} />
        </button>
        <button
          data-tooltip="Clear canvas"
          onClick={() => void clearCanvas()}
          className={ICON_BTN}
          disabled={nodes.length === 0}
        >
          <EraserIcon size={15} />
        </button>
    </div>
  );
}
