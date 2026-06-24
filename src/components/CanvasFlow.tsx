import { useCallback, useEffect, useState } from "react";
import {
  Background,
  BackgroundVariant,
  ControlButton,
  Controls,
  MiniMap,
  ReactFlow,
  useReactFlow,
  useStore,
  useStoreApi,
  type NodeTypes,
} from "@xyflow/react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { NODE_W, useCanvasStore } from "../stores/canvasStore";
import { useVpsStore } from "../stores/vpsStore";
import { onCanvasCommand } from "../lib/tauri";
import { TerminalNode } from "./TerminalNode";
import { SftpNode } from "./SftpNode";
import { FloatingEdge } from "./FloatingEdge";
import { LockIcon, LockOpenIcon, RadarIcon } from "./icons";
import { VPS_DND_MIME } from "./ServerPanel";

const nodeTypes: NodeTypes = { terminal: TerminalNode, sftp: SftpNode };
const edgeTypes = { floating: FloatingEdge };

/** Executes canvas actions the agent requests (open/close nodes, tile). Lives
 *  inside <ReactFlow> so it has the pane dimensions + viewport controls. */
function CanvasCommandBridge() {
  const addVps = useCanvasStore((s) => s.addVps);
  const addSftp = useCanvasStore((s) => s.addSftp);
  const arrangeTiles = useCanvasStore((s) => s.arrangeTiles);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const { setViewport } = useReactFlow();
  const paneW = useStore((s) => s.width);
  const paneH = useStore((s) => s.height);

  // In tile mode, keep the grid filling the canvas: re-arrange whenever the pane
  // resizes (e.g. opening/closing the agent chat or a sidebar) and pin zoom to 1.
  useEffect(() => {
    if (layoutMode !== "tile" || !paneW || !paneH) return;
    arrangeTiles({ width: paneW, height: paneH });
    setViewport({ x: 0, y: 0, zoom: 1 });
  }, [layoutMode, paneW, paneH, arrangeTiles, setViewport]);

  useEffect(() => {
    let un: UnlistenFn | undefined;
    onCanvasCommand((cmd) => {
      const vps = cmd.vps_id
        ? useVpsStore.getState().vpsList.find((v) => v.id === cmd.vps_id)
        : undefined;
      switch (cmd.action) {
        case "open_terminal":
          if (vps) addVps(vps);
          break;
        case "open_sftp":
          if (vps) addSftp(vps);
          break;
        case "tile":
          arrangeTiles({ width: paneW, height: paneH });
          setViewport({ x: 0, y: 0, zoom: 1 }, { duration: 300 });
          break;
        case "close":
          if (cmd.node_id) {
            removeNode(cmd.node_id);
          } else if (cmd.vps_id) {
            useCanvasStore
              .getState()
              .nodes.filter((n) => n.data.vpsId === cmd.vps_id)
              .forEach((n) => removeNode(n.id));
          }
          break;
        // "reconnect" is handled inside each TerminalNode (it owns the SSH session).
      }
    }).then((u) => (un = u));
    return () => un?.();
  }, [addVps, addSftp, arrangeTiles, removeNode, setViewport, paneW, paneH]);

  return null;
}

/** Horizontal canvas controls: zoom/fit + a custom lock toggle + a radar (minimap)
 *  show/hide toggle. */
function CanvasControls({
  miniMap,
  onToggleMiniMap,
}: {
  miniMap: boolean;
  onToggleMiniMap: () => void;
}) {
  const store = useStoreApi();
  const interactive = useStore((s) => s.nodesDraggable);
  const toggleInteractive = () => {
    const next = !store.getState().nodesDraggable;
    store.setState({
      nodesDraggable: next,
      nodesConnectable: next,
      elementsSelectable: next,
    });
  };
  return (
    <Controls
      orientation="horizontal"
      showInteractive={false}
      className="!flex-row !border-[var(--border)] !bg-[var(--surface)]"
    >
      <ControlButton
        onClick={toggleInteractive}
        data-tooltip={interactive ? "Lock the canvas" : "Unlock the canvas"}
      >
        {interactive ? <LockOpenIcon size={13} /> : <LockIcon size={13} />}
      </ControlButton>
      <ControlButton
        onClick={onToggleMiniMap}
        data-tooltip={miniMap ? "Hide the radar map" : "Show the radar map"}
      >
        <RadarIcon size={13} />
      </ControlButton>
    </Controls>
  );
}

export function CanvasFlow() {
  const nodes = useCanvasStore((s) => s.nodes);
  const edges = useCanvasStore((s) => s.edges);
  const onNodesChange = useCanvasStore((s) => s.onNodesChange);
  const onEdgesChange = useCanvasStore((s) => s.onEdgesChange);
  const onConnect = useCanvasStore((s) => s.onConnect);
  const addVps = useCanvasStore((s) => s.addVps);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const snapGrid = useCanvasStore((s) => s.snapGrid);
  const [showMiniMap, setShowMiniMap] = useState(true);
  const { screenToFlowPosition } = useReactFlow();

  // Tile mode is a fixed full-canvas grid: lock zoom/pan and free the corners.
  const tiled = layoutMode === "tile";

  const onDragOver = useCallback((e: React.DragEvent) => {
    if (e.dataTransfer.types.includes(VPS_DND_MIME)) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    }
  }, []);

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      const vpsId = e.dataTransfer.getData(VPS_DND_MIME);
      if (!vpsId) return;
      e.preventDefault();
      const vps = useVpsStore.getState().vpsList.find((v) => v.id === vpsId);
      if (!vps) return;
      // Drop the terminal centered on the cursor.
      const p = screenToFlowPosition({ x: e.clientX, y: e.clientY });
      addVps(vps, { x: p.x - NODE_W / 2, y: p.y - 24 });
    },
    [addVps, screenToFlowPosition],
  );

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      onDrop={onDrop}
      onDragOver={onDragOver}
      snapToGrid={layoutMode === "snap"}
      snapGrid={snapGrid}
      minZoom={0.05}
      maxZoom={2}
      zoomOnScroll={!tiled}
      zoomOnPinch={!tiled}
      zoomOnDoubleClick={!tiled}
      panOnDrag={!tiled}
      // Keep all terminal nodes mounted so sessions & scrollback survive panning.
      onlyRenderVisibleElements={false}
      deleteKeyCode={null}
      proOptions={{ hideAttribution: true }}
      fitView
      fitViewOptions={{ padding: 0.2 }}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1} color="#1a2233" />
      {!tiled && showMiniMap && (
        <MiniMap
          pannable
          zoomable
          nodeColor={() => "#243049"}
          nodeStrokeColor={() => "#3b82f6"}
          maskColor="rgba(5,8,13,0.7)"
          className="!bg-[var(--bg)]"
        />
      )}
      {!tiled && (
        <CanvasControls miniMap={showMiniMap} onToggleMiniMap={() => setShowMiniMap((v) => !v)} />
      )}
      <CanvasCommandBridge />
    </ReactFlow>
  );
}
