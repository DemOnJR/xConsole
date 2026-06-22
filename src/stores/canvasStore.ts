import { create } from "zustand";
import { persist } from "zustand/middleware";
import {
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
  type Viewport,
} from "@xyflow/react";
import type { Vps } from "../lib/tauri";

export type LayoutMode = "freeform" | "snap" | "tile";

export interface TermData {
  vpsId: string;
  name: string;
  host: string;
  [key: string]: unknown;
}

export interface SftpData {
  vpsId: string;
  name: string;
  host: string;
  /** Canvas node id of linked SSH terminal. */
  linkedTerminalId?: string;
  /** When linked, auto-navigate SFTP to terminal cwd. */
  followTerminal?: boolean;
  [key: string]: unknown;
}

export type CanvasEdge = Edge<{ kind: "sftp-terminal" }>;

export type TermNode = Node<TermData, "terminal">;
export type SftpNode = Node<SftpData, "sftp">;
export type CanvasNode = TermNode | SftpNode;

export const NODE_W = 460;
export const NODE_H = 320;
const GAP = 24;
const SNAP_GRID: [number, number] = [20, 20];
/** Max terminals using the WebGL renderer at once (webview context limit ~16). */
const MAX_WEBGL = 4;

interface CanvasState {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  layoutMode: LayoutMode;
  focusedId: string | null;
  /** LRU of node ids permitted to use the WebGL renderer (front = most recent). */
  webglIds: string[];

  snapGrid: [number, number];
  setNodes: (nodes: CanvasNode[]) => void;
  setEdges: (edges: CanvasEdge[]) => void;
  onNodesChange: (changes: NodeChange<CanvasNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<CanvasEdge>[]) => void;
  onConnect: (connection: Connection) => void;
  updateNodeData: (id: string, partial: Partial<SftpData>) => void;
  addVps: (vps: Vps, position?: { x: number; y: number }) => string;
  addSftp: (vps: Vps, position?: { x: number; y: number }) => string;
  removeNode: (id: string) => void;
  setLayout: (mode: LayoutMode) => void;
  focus: (id: string | null) => void;
  isWebgl: (id: string) => boolean;
  arrangeTiles: () => void;
  clear: () => void;
}

export const useCanvasStore = create<CanvasState>()(
  persist(
    (set, get) => ({
      nodes: [],
      edges: [],
      layoutMode: "freeform",
      focusedId: null,
      webglIds: [],
      snapGrid: SNAP_GRID,

      setNodes: (nodes) => set({ nodes }),
      setEdges: (edges) => set({ edges }),

      onNodesChange: (changes) =>
        set((s) => ({ nodes: applyNodeChanges(changes, s.nodes) })),

      onEdgesChange: (changes) => {
        const removedIds = changes
          .filter((c) => c.type === "remove")
          .map((c) => c.id);
        set((s) => {
          let nodes = s.nodes;
          if (removedIds.length > 0) {
            const removed = s.edges.filter((e) => removedIds.includes(e.id));
            const unlinkedSftpIds = new Set(
              removed.map((e) => e.source).filter(Boolean) as string[],
            );
            if (unlinkedSftpIds.size > 0) {
              nodes = s.nodes.map((n) =>
                unlinkedSftpIds.has(n.id) && n.type === "sftp"
                  ? {
                      ...n,
                      data: {
                        ...n.data,
                        linkedTerminalId: undefined,
                        followTerminal: false,
                      },
                    }
                  : n,
              );
            }
          }
          return {
            nodes,
            edges: applyEdgeChanges(changes, s.edges),
          };
        });
      },

      onConnect: (connection) => {
        const { source, target } = connection;
        if (!source || !target) return;
        const nodes = get().nodes;
        const src = nodes.find((n) => n.id === source);
        const tgt = nodes.find((n) => n.id === target);
        if (!src || !tgt) return;

        // SFTP (source) → Terminal (target): path flows terminal → sftp data
        let terminalId = target;
        let sftpId = source;
        if (src.type === "terminal" && tgt.type === "sftp") {
          terminalId = source;
          sftpId = target;
        } else if (src.type !== "sftp" || tgt.type !== "terminal") {
          return;
        }

        const sftpNode = nodes.find((n) => n.id === sftpId);
        if (!sftpNode || sftpNode.type !== "sftp") return;

        const edgeId = `link-${terminalId}-${sftpId}`;
        set((s) => ({
          edges: [
            ...s.edges.filter((e) => e.source !== sftpId),
            {
              id: edgeId,
              source: sftpId,
              target: terminalId,
              type: "smoothstep",
              animated: true,
              style: { stroke: "#22d3ee", strokeWidth: 2 },
              data: { kind: "sftp-terminal" },
            },
          ],
          nodes: s.nodes.map((n) =>
            n.id === sftpId
              ? {
                  ...n,
                  data: {
                    ...n.data,
                    linkedTerminalId: terminalId,
                    followTerminal: true,
                  },
                }
              : n,
          ),
        }));
      },

      updateNodeData: (id, partial) =>
        set((s) => ({
          nodes: s.nodes.map((n) =>
            n.id === id ? { ...n, data: { ...n.data, ...partial } } : n,
          ),
        })),

      addVps: (vps, position) => {
        const id = crypto.randomUUID();
        const count = get().nodes.length;
        // Cascade new nodes so they don't stack exactly.
        const pos =
          position ?? {
            x: 80 + (count % 4) * (NODE_W + GAP),
            y: 80 + Math.floor(count / 4) * (NODE_H + GAP),
          };
        const node: TermNode = {
          id,
          type: "terminal",
          position: pos,
          width: NODE_W,
          height: NODE_H,
          data: { vpsId: vps.id, name: vps.name, host: vps.host },
        };
        set((s) => ({ nodes: [...s.nodes, node] }));
        get().focus(id);
        return id;
      },

      addSftp: (vps, position) => {
        const id = crypto.randomUUID();
        const count = get().nodes.length;
        const pos =
          position ?? {
            x: 80 + (count % 4) * (NODE_W + GAP),
            y: 80 + Math.floor(count / 4) * (NODE_H + GAP),
          };
        const node: SftpNode = {
          id,
          type: "sftp",
          position: pos,
          width: NODE_W,
          height: NODE_H,
          data: { vpsId: vps.id, name: vps.name, host: vps.host },
        };
        set((s) => ({ nodes: [...s.nodes, node] }));
        get().focus(id);
        return id;
      },

      removeNode: (id) =>
        set((s) => ({
          nodes: s.nodes.filter((n) => n.id !== id),
          edges: s.edges.filter((e) => e.source !== id && e.target !== id),
          webglIds: s.webglIds.filter((w) => w !== id),
          focusedId: s.focusedId === id ? null : s.focusedId,
        })),

      setLayout: (mode) => {
        set({ layoutMode: mode });
        if (mode === "tile") get().arrangeTiles();
      },

      focus: (id) =>
        set((s) => {
          if (!id) return { focusedId: null };
          const webglIds = [id, ...s.webglIds.filter((w) => w !== id)].slice(
            0,
            MAX_WEBGL,
          );
          return { focusedId: id, webglIds };
        }),

      isWebgl: (id) => get().webglIds.includes(id),

      arrangeTiles: () =>
        set((s) => {
          const n = s.nodes.length;
          if (n === 0) return {};
          const cols = Math.ceil(Math.sqrt(n));
          const nodes = s.nodes.map((node, i) => {
            const col = i % cols;
            const row = Math.floor(i / cols);
            const w = (node.width as number) || NODE_W;
            const h = (node.height as number) || NODE_H;
            return {
              ...node,
              position: { x: col * (w + GAP) + GAP, y: row * (h + GAP) + GAP },
            };
          });
          return { nodes };
        }),

      clear: () => set({ nodes: [], edges: [], webglIds: [], focusedId: null }),
    }),
    {
      name: "xconsole-canvas",
      version: 1,
      partialize: (state) => ({ layoutMode: state.layoutMode }),
    },
  ),
);

export const defaultViewport: Viewport = { x: 0, y: 0, zoom: 1 };
