import { useReactFlow } from "@xyflow/react";
import { useCanvasStore } from "../stores/canvasStore";
import { useWorkspaceStore } from "../stores/workspaceStore";

/**
 * Returns a function that opens a workspace by id. Re-opening the active
 * workspace just re-fits the view (it must NOT recreate terminals, which would
 * kill live sessions). Switching to another workspace restores its nodes;
 * matching node ids are reattached to their background sessions by TerminalNode.
 */
export function useOpenWorkspace() {
  const restore = useWorkspaceStore((s) => s.restore);
  const setNodes = useCanvasStore((s) => s.setNodes);
  const setEdges = useCanvasStore((s) => s.setEdges);
  const setLayout = useCanvasStore((s) => s.setLayout);
  const { setViewport, fitView } = useReactFlow();

  return async (id: string) => {
    if (id === useWorkspaceStore.getState().activeId) {
      fitView({ duration: 300, padding: 0.2 });
      return;
    }
    const res = await restore(id);
    if (!res) return;
    setNodes(res.nodes);
    setEdges(res.edges);
    setLayout(res.layout);
    setTimeout(() => {
      setViewport(res.viewport, { duration: 300 });
      if (res.nodes.length) fitView({ duration: 300, padding: 0.2 });
    }, 60);
  };
}
