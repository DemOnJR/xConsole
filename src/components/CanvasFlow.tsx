import { useCallback } from "react";
import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  useReactFlow,
  type NodeTypes,
} from "@xyflow/react";
import { NODE_W, useCanvasStore } from "../stores/canvasStore";
import { useVpsStore } from "../stores/vpsStore";
import { TerminalNode } from "./TerminalNode";
import { SftpNode } from "./SftpNode";
import { Toolbar } from "./Toolbar";
import { VPS_DND_MIME } from "./ServerPanel";

const nodeTypes: NodeTypes = { terminal: TerminalNode, sftp: SftpNode };

export function CanvasFlow() {
  const nodes = useCanvasStore((s) => s.nodes);
  const edges = useCanvasStore((s) => s.edges);
  const onNodesChange = useCanvasStore((s) => s.onNodesChange);
  const onEdgesChange = useCanvasStore((s) => s.onEdgesChange);
  const onConnect = useCanvasStore((s) => s.onConnect);
  const addVps = useCanvasStore((s) => s.addVps);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const snapGrid = useCanvasStore((s) => s.snapGrid);
  const { screenToFlowPosition } = useReactFlow();

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
      onDrop={onDrop}
      onDragOver={onDragOver}
      snapToGrid={layoutMode === "snap"}
      snapGrid={snapGrid}
      minZoom={0.05}
      maxZoom={2}
      // Keep all terminal nodes mounted so sessions & scrollback survive panning.
      onlyRenderVisibleElements={false}
      deleteKeyCode={null}
      proOptions={{ hideAttribution: true }}
      fitView
      fitViewOptions={{ padding: 0.2 }}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1} color="#1a2233" />
      <MiniMap
        pannable
        zoomable
        nodeColor={() => "#243049"}
        nodeStrokeColor={() => "#3b82f6"}
        maskColor="rgba(5,8,13,0.7)"
        className="!bg-[#0b0f17]"
      />
      <Controls className="!bg-[#11161f] !border-[#1f2737]" />
      <Toolbar />
    </ReactFlow>
  );
}
