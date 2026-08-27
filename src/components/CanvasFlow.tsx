import { useEffect, useRef, useState } from "react";
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
  type Node,
} from "@xyflow/react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { NODE_W, useCanvasStore } from "../stores/canvasStore";
import { useVpsStore } from "../stores/vpsStore";
import { onCanvasCommand, onCanvasPreview } from "../lib/tauri";
import { TerminalNode } from "./TerminalNode";
import { AgentNodeView } from "./agent/AgentNode";
import { DynamicPluginNode } from "./plugins/DynamicPluginNode";
import { GoalNode } from "./GoalNode";
import { PreviewNode } from "./PreviewNode";
import { FloatingEdge } from "./FloatingEdge";
import { LockIcon, LockOpenIcon, RadarIcon } from "./icons";
import { onInternalDrop } from "../stores/dragStore";
import { useSnapDragStore, endSnapDrag } from "../lib/snapDrag";
import { SnapPreview } from "./SnapPreview";

const nodeTypes: NodeTypes = {
  terminal: TerminalNode,
  agent: AgentNodeView,
  sftp: DynamicPluginNode as any,
  db: DynamicPluginNode as any,
  goal: GoalNode,
  preview: PreviewNode,
};
const edgeTypes = { floating: FloatingEdge };

/** Executes canvas actions the agent requests (open/close nodes, tile, open previews). Lives
 *  inside <ReactFlow> so it has the pane dimensions + viewport controls. */
function CanvasCommandBridge() {
  const addVps = useCanvasStore((s) => s.addVps);
  const addSftp = useCanvasStore((s) => s.addSftp);
  const addPreview = useCanvasStore((s) => s.addPreview);
  const retileFromPositions = useCanvasStore((s) => s.retileFromPositions);
  const setPaneSize = useCanvasStore((s) => s.setPaneSize);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const { setViewport } = useReactFlow();
  const paneW = useStore((s) => s.width);
  const paneH = useStore((s) => s.height);

  // Publish the pane size to the store, which re-tiles on its own when the mode is
  // "tile". Keeping the measurement here (React Flow owns it) and the arrangement
  // there means keyboard layout edits can re-tile without going through this effect.
  useEffect(() => {
    if (!paneW || !paneH) return;
    setPaneSize({ width: paneW, height: paneH });
  }, [paneW, paneH, setPaneSize]);

  // Tiles are laid out in flow coordinates starting at the origin, so the viewport
  // has to sit at 1:1 for them to line up with the pane.
  useEffect(() => {
    if (layoutMode !== "tile") return;
    setViewport({ x: 0, y: 0, zoom: 1 });
  }, [layoutMode, paneW, paneH, setViewport]);

  useEffect(() => {
    let unCmd: UnlistenFn | undefined;
    let unPrev: UnlistenFn | undefined;

    onCanvasPreview((p) => {
      addPreview({
        id: p.id,
        title: p.title,
        html: p.html,
        width: p.width,
        height: p.height,
      });
    }).then((u) => (unPrev = u));

    onCanvasCommand((cmd) => {
      const vps = cmd.vps_id
        ? useVpsStore.getState().vpsList.find((v) => v.id === cmd.vps_id)
        : undefined;
      switch (cmd.action) {
        case "open_terminal":
          if (vps) {
            const existing = useCanvasStore
              .getState()
              .nodes.find((n) => n.type === "terminal" && String(n.data.vpsId) === vps.id);
            if (existing) {
              useCanvasStore.getState().focus(existing.id);
            } else {
              addVps(vps);
            }
          }
          break;
        case "open_sftp":
          if (vps) addSftp(vps);
          break;
        case "tile":
          retileFromPositions({ width: paneW, height: paneH });
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
    }).then((u) => (unCmd = u));

    return () => {
      unCmd?.();
      unPrev?.();
    };
  }, [addVps, addSftp, addPreview, retileFromPositions, removeNode, setViewport, paneW, paneH]);

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
  const [showMiniMap, setShowMiniMap] = useState(true);
  const { screenToFlowPosition } = useReactFlow();

  // Tile mode is a fixed full-canvas grid: lock zoom/pan and free the corners.
  const tiled = layoutMode === "tile";

  /** Windows-style snap preview: while a node is dragged (freeform OR tile mode),
   *  track the cursor in pane fractions so the overlay can highlight the zone under
   *  it. The preview only arms when the cursor is near a zone (see snapDrag). */
  const paneRect = useRef<DOMRect | null>(null);
  const onNodeDragStart = (_: MouseEvent | TouchEvent, node: Node) => {
    const state = useSnapDragStore.getState();
    if (!state.nodeId) state.begin(node.id);
    paneRect.current =
      document.querySelector<HTMLElement>(".react-flow__pane")?.getBoundingClientRect() ?? null;
  };
  const onNodeDrag = (_: MouseEvent | TouchEvent) => {
    const rect = paneRect.current;
    if (!rect) return;
    const clientX = "clientX" in _ ? _.clientX : 0;
    const clientY = "clientY" in _ ? _.clientY : 0;
    useSnapDragStore.getState().move(
      (clientX - rect.left) / rect.width,
      (clientY - rect.top) / rect.height,
    );
  };

  const onNodeDragStop = () => {
    paneRect.current = null;
    endSnapDrag();
  };

  // A server dropped on the canvas becomes a terminal. The drag is delivered by the
  // pointer-event system (see dragStore) rather than HTML5 DnD, which the webview stops
  // firing once Tauri intercepts native drags to receive files.
  useEffect(() => {
    const un = onInternalDrop("canvas", (payload, x, y) => {
      if (payload.kind !== "vps") return;
      const vps = useVpsStore.getState().vpsList.find((v) => v.id === payload.vpsId);
      if (!vps) return;
      const p = screenToFlowPosition({ x, y });
      const snapHint = useSnapDragStore.getState().hint;
      const curLayoutMode = useCanvasStore.getState().layoutMode;

      if (curLayoutMode === "tile" || snapHint) {
        const id = addVps(vps, { x: p.x - NODE_W / 2, y: p.y - 24 });
        if (snapHint) {
          endSnapDrag(id);
        }
      } else {
        addVps(vps, { x: p.x - NODE_W / 2, y: p.y - 24 });
      }
    });
    return un;
  }, [addVps, screenToFlowPosition]);

  return (
    <div className="h-full w-full" data-drop="canvas">
    <ReactFlow
      nodes={nodes}
      edges={edges}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      onNodeDragStart={onNodeDragStart}
      onNodeDrag={onNodeDrag}
      onNodeDragStop={onNodeDragStop}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
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
      // No fitView. The saved viewport is restored verbatim on launch, and fitting
      // would silently overrule it — which is the "everything got rearranged when I
      // reopened the app" complaint.
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
      <SnapPreview />
      <CanvasCommandBridge />
    </ReactFlow>
    </div>
  );
}
