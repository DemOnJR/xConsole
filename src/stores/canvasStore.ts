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
import {
  applyRowCounts,
  autoLayout,
  computeBoxes,
  moveToRow,
  moveWithinRow,
  reconcile,
  resizeRow,
  rowsFromPositions,
  resizeTile,
  toggleFullWidth,
  type TileLayout,
} from "../lib/tileLayout";

// "snap" was removed: it snapped node positions to a grid while dragging, which was
// neither freeform nor a real tiling, and nobody used it.
export type LayoutMode = "freeform" | "tile";

/** Which way a tile move goes: within its row, or to the row above/below. */
export type TileMoveAxis = "horizontal" | "vertical";

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

/** A database browser node. Connection state lives in the node, not the store, because
 *  it holds a session id that must not outlive the component. */
export interface DbData {
  vpsId: string;
  name: string;
  host: string;
  [key: string]: unknown;
}

export type CanvasEdge = Edge<{ kind: "sftp-terminal" }>;

export type TermNode = Node<TermData, "terminal">;
export type SftpNode = Node<SftpData, "sftp">;
export type DbNode = Node<DbData, "db">;
export type CanvasNode = TermNode | SftpNode | DbNode;

export const NODE_W = 460;
export const NODE_H = 320;
const GAP = 24;
/** Max terminals using the WebGL renderer at once (webview context limit ~16). */
const MAX_WEBGL = 4;

interface CanvasState {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  layoutMode: LayoutMode;
  focusedId: string | null;
  /** LRU of node ids permitted to use the WebGL renderer (front = most recent). */
  webglIds: string[];

  /**
   * The tile arrangement: which nodes sit in which row, and their relative sizes.
   * `null` means "follow the balanced default" and is re-derived on every change.
   * Node ids only — the `nodes` array is never reordered, because its index is baked
   * into the saved workspace node id.
   */
  tileLayout: TileLayout | null;
  /** Last known canvas pane size, so layout edits can re-tile without the caller. */
  paneSize: { width: number; height: number } | null;

  setNodes: (nodes: CanvasNode[]) => void;
  setEdges: (edges: CanvasEdge[]) => void;
  onNodesChange: (changes: NodeChange<CanvasNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<CanvasEdge>[]) => void;
  onConnect: (connection: Connection) => void;
  updateNodeData: (id: string, partial: Partial<SftpData>) => void;
  addVps: (vps: Vps, position?: { x: number; y: number }) => string;
  addSftp: (vps: Vps, position?: { x: number; y: number }) => string;
  /** Drop a database browser for this server onto the canvas. */
  addDb: (vps: Vps, position?: { x: number; y: number }) => string;
  removeNode: (id: string) => void;
  setLayout: (mode: LayoutMode) => void;
  focus: (id: string | null) => void;
  isWebgl: (id: string) => boolean;
  /** Arrange nodes into the current tile layout. With `dims` (the canvas pane size in
   *  px) every node is resized so the rows fill the window edge-to-edge. */
  arrangeTiles: (dims?: { width: number; height: number }) => void;
  /** Record the live pane size so layout edits can re-tile on their own. */
  setPaneSize: (dims: { width: number; height: number }) => void;
  /**
   * Re-tile, taking the arrangement from where the nodes currently sit. This is what
   * the Tile button does: drag terminals roughly into place, press it, and the grid
   * adopts that shape (three side by side become one row of three).
   */
  retileFromPositions: (dims?: { width: number; height: number }) => void;
  /** Install an arrangement wholesale (workspace restore, or after an id rebind). */
  setTileLayout: (layout: TileLayout | null) => void;
  /** Re-flow into rows of the given sizes, e.g. `[3, 2]` for 3 on top, 2 below. */
  setTileRows: (counts: number[]) => void;
  /** Discard any hand-tuned arrangement and go back to the balanced default. */
  resetTileLayout: () => void;
  /** Move a tile within its row (`horizontal`) or between rows (`vertical`). */
  moveTile: (id: string, dir: -1 | 1, axis: TileMoveAxis) => void;
  /** Grow/shrink a tile's width share, or its row's height share. */
  growTile: (id: string, delta: number, axis: TileMoveAxis) => void;
  /** Give a tile its own full-width row — or merge it back. */
  toggleTileFullWidth: (id: string) => void;
  clear: () => void;
}

/** The slice of state `applyTiles` reads — keeps the helper testable and cheap. */
type TileInput = Pick<CanvasState, "nodes" | "tileLayout">;

/**
 * Reconcile the layout against the live nodes and write the resulting geometry back
 * onto them. Returns a state patch, so every layout action is a single `set`.
 *
 * With `dims` the rows fill the pane exactly. Without (no pane measured yet) nodes are
 * merely flowed into their rows at their current size, which keeps the arrangement
 * meaningful until the canvas reports its size.
 */
function applyTiles(
  s: TileInput,
  dims?: { width: number; height: number },
): Partial<CanvasState> {
  if (s.nodes.length === 0) return { tileLayout: null };

  const layout = reconcile(s.tileLayout, s.nodes.map((n) => n.id));

  if (dims && dims.width > 0 && dims.height > 0) {
    const boxes = new Map(computeBoxes(layout, dims.width, dims.height).map((b) => [b.id, b]));
    const nodes = s.nodes.map((node) => {
      const box = boxes.get(node.id);
      if (!box) return node;
      return {
        ...node,
        position: { x: box.x, y: box.y },
        width: box.width,
        height: box.height,
      };
    });
    return { nodes, tileLayout: layout };
  }

  // No pane size yet: flow each row left-to-right at the nodes' existing sizes.
  const byId = new Map(s.nodes.map((n) => [n.id, n]));
  const pos = new Map<string, { x: number; y: number }>();
  let y = GAP;
  for (const row of layout.rows) {
    let x = GAP;
    let tallest = 0;
    for (const item of row.items) {
      const node = byId.get(item.id);
      if (!node) continue;
      const w = (node.width as number) || NODE_W;
      const h = (node.height as number) || NODE_H;
      pos.set(item.id, { x, y });
      x += w + GAP;
      tallest = Math.max(tallest, h);
    }
    y += (tallest || NODE_H) + GAP;
  }
  const nodes = s.nodes.map((node) => {
    const p = pos.get(node.id);
    return p ? { ...node, position: p } : node;
  });
  return { nodes, tileLayout: layout };
}

export const useCanvasStore = create<CanvasState>()(
  persist(
    (set, get) => ({
      nodes: [],
      edges: [],
      layoutMode: "freeform",
      focusedId: null,
      webglIds: [],
      tileLayout: null,
      paneSize: null,

      setNodes: (nodes) => set({ nodes }),
      setEdges: (edges) => set({ edges }),

      onNodesChange: (changes) =>
        set((s) => {
          // In tile mode a node's size is derived from the layout, so writing pixel
          // dimensions straight onto the node does nothing lasting — the next reflow
          // overwrites them. Dragging an edge has to become a change to the *layout*.
          if (s.layoutMode === "tile" && s.tileLayout && s.paneSize) {
            // `resizing` marks a live NodeResizer drag. React Flow reports its own
            // measurements as dimension changes too, and those must not be read as user
            // intent: on the first one a node has no width yet, so the "delta" would be
            // the node's entire width and the layout would be thrown across the pane.
            const resizes = changes.filter(
              (c): c is Extract<NodeChange<CanvasNode>, { type: "dimensions" }> =>
                c.type === "dimensions" && !!c.dimensions && c.resizing === true,
            );
            if (resizes.length > 0) {
              let layout = reconcile(s.tileLayout, s.nodes.map((n) => n.id));
              for (const c of resizes) {
                const node = s.nodes.find((n) => n.id === c.id);
                if (!node) continue;
                const dw = c.dimensions!.width - ((node.width as number) ?? 0);
                const dh = c.dimensions!.height - ((node.height as number) ?? 0);
                // Weights are relative within a row, so a pixel delta becomes a weight
                // delta scaled by how much of the pane that row/column spans.
                const row = layout.rows.find((r) => r.items.some((i) => i.id === c.id));
                if (!row) continue;
                if (Math.abs(dw) >= 1) {
                  const total = row.items.reduce((a, i) => a + i.weight, 0);
                  layout = resizeTile(layout, c.id, (dw / s.paneSize.width) * total);
                }
                if (Math.abs(dh) >= 1) {
                  const total = layout.rows.reduce((a, r) => a + r.weight, 0);
                  layout = resizeRow(layout, c.id, (dh / s.paneSize.height) * total);
                }
              }
              // Pass the pane size. Without it applyTiles falls back to flowing rows at
              // their current sizes with a GAP between them, instead of filling the pane
              // edge to edge — which is what put a large gap between every window.
              return applyTiles({ ...s, tileLayout: layout }, s.paneSize);
            }
          }
          return { ...s, nodes: applyNodeChanges(changes, s.nodes) };
        }),

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
              type: "floating",
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
        // In tile mode a new node has to join the grid straight away, or it lands on
        // the cascade position on top of the tiles until the user re-tiles by hand.
        if (get().layoutMode === "tile") get().arrangeTiles();
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
        if (get().layoutMode === "tile") get().arrangeTiles();
        get().focus(id);
        return id;
      },

      addDb: (vps, position) => {
        const id = crypto.randomUUID();
        const count = get().nodes.length;
        const pos =
          position ?? {
            x: 80 + (count % 4) * (NODE_W + GAP),
            y: 80 + Math.floor(count / 4) * (NODE_H + GAP),
          };
        const node: DbNode = {
          id,
          type: "db",
          position: pos,
          // Wider by default: a data grid needs room in a way a terminal doesn't.
          width: Math.round(NODE_W * 1.4),
          height: NODE_H,
          data: { vpsId: vps.id, name: vps.name, host: vps.host },
        };
        set((s) => ({ nodes: [...s.nodes, node] }));
        if (get().layoutMode === "tile") get().arrangeTiles();
        get().focus(id);
        return id;
      },

      removeNode: (id) => {
        set((s) => ({
          // Drop the node, and unlink any SFTP node that followed it so it isn't
          // left with a dangling linkedTerminalId / followTerminal=true.
          nodes: s.nodes
            .filter((n) => n.id !== id)
            .map((n) =>
              n.type === "sftp" && n.data.linkedTerminalId === id
                ? { ...n, data: { ...n.data, linkedTerminalId: undefined, followTerminal: false } }
                : n,
            ),
          edges: s.edges.filter((e) => e.source !== id && e.target !== id),
          webglIds: s.webglIds.filter((w) => w !== id),
          focusedId: s.focusedId === id ? null : s.focusedId,
        }));
        // Close the hole the removed tile left behind.
        if (get().layoutMode === "tile") get().arrangeTiles();
      },

      setLayout: (mode) => {
        set({ layoutMode: mode });
        // Adopt however the nodes are arranged right now. Applying the *stored* layout
        // here would snap them into the previous grid first, and anything the user had
        // just dragged would be gone before it could be read.
        if (mode === "tile") get().retileFromPositions();
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

      arrangeTiles: (dims) =>
        set((s) => applyTiles(s, dims ?? s.paneSize ?? undefined)),

      setPaneSize: (dims) =>
        set((s) => {
          if (
            s.paneSize &&
            s.paneSize.width === dims.width &&
            s.paneSize.height === dims.height
          ) {
            return {};
          }
          // Re-tile against the new pane immediately, so a panel toggle or window
          // resize reflows in the same commit rather than a frame later.
          return s.layoutMode === "tile"
            ? { paneSize: dims, ...applyTiles(s, dims) }
            : { paneSize: dims };
        }),

      setTileLayout: (layout) => set({ tileLayout: layout }),

      retileFromPositions: (dims) =>
        set((s) => {
          if (s.nodes.length === 0) return {};
          const derived = rowsFromPositions(
            s.nodes.map((n) => ({
              id: n.id,
              x: n.position.x,
              y: n.position.y,
              width: (n.width as number) || NODE_W,
              height: (n.height as number) || NODE_H,
            })),
          );
          return applyTiles({ ...s, tileLayout: derived }, dims ?? s.paneSize ?? undefined);
        }),

      setTileRows: (counts) =>
        set((s) => {
          const base = reconcile(s.tileLayout, s.nodes.map((n) => n.id));
          return applyTiles({ ...s, tileLayout: applyRowCounts(base, counts) }, s.paneSize ?? undefined);
        }),

      resetTileLayout: () =>
        set((s) => applyTiles({ ...s, tileLayout: autoLayout(s.nodes.map((n) => n.id)) }, s.paneSize ?? undefined)),

      moveTile: (id, dir, axis) =>
        set((s) => {
          const base = reconcile(s.tileLayout, s.nodes.map((n) => n.id));
          const next =
            axis === "horizontal" ? moveWithinRow(base, id, dir) : moveToRow(base, id, dir);
          return applyTiles({ ...s, tileLayout: next }, s.paneSize ?? undefined);
        }),

      growTile: (id, delta, axis) =>
        set((s) => {
          const base = reconcile(s.tileLayout, s.nodes.map((n) => n.id));
          const next =
            axis === "horizontal" ? resizeTile(base, id, delta) : resizeRow(base, id, delta);
          return applyTiles({ ...s, tileLayout: next }, s.paneSize ?? undefined);
        }),

      toggleTileFullWidth: (id) =>
        set((s) => {
          const base = reconcile(s.tileLayout, s.nodes.map((n) => n.id));
          return applyTiles({ ...s, tileLayout: toggleFullWidth(base, id) }, s.paneSize ?? undefined);
        }),

      clear: () =>
        set({ nodes: [], edges: [], webglIds: [], focusedId: null, tileLayout: null }),
    }),
    {
      name: "xconsole-canvas",
      version: 1,
      // The arrangement rides along with the mode so a hand-tuned grid survives a
      // restart. It references node ids that may be gone by then; `reconcile` drops
      // stale ids and places new ones on the next tile.
      partialize: (state) => ({
        layoutMode: state.layoutMode,
        tileLayout: state.tileLayout,
      }),
    },
  ),
);

export const defaultViewport: Viewport = { x: 0, y: 0, zoom: 1 };
