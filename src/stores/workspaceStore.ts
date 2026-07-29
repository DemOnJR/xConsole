import { create } from "zustand";
import type { Viewport } from "@xyflow/react";
import { api, type Workspace, type WorkspaceProject } from "../lib/tauri";
import {
  defaultViewport,
  useCanvasStore,
  type CanvasEdge,
  type CanvasNode,
  type LayoutMode,
} from "./canvasStore";
import { useSessionStore } from "./sessionStore";
import { reconcile, type TileLayout } from "../lib/tileLayout";

/** Deterministic node id for a workspace slot (stable across reopen). */
export const workspaceNodeId = (workspaceId: string, index: number) =>
  `${workspaceId}::${index}`;

/** Serialized node persisted in a workspace (no live session state). */
interface SavedNode {
  /** Legacy: persisted node id. Restore now derives a deterministic id by slot. */
  id?: string;
  vpsId: string;
  name: string;
  host: string;
  x: number;
  y: number;
  width: number;
  height: number;
  nodeType?: "terminal" | "sftp";
  linkedTerminalIndex?: number;
  followTerminal?: boolean;
}

interface SavedEdge {
  sourceIndex: number;
  targetIndex: number;
}

/**
 * The tile arrangement, stored by node *index* rather than id.
 *
 * Restore derives node ids from the workspace id and the slot index
 * (`workspaceNodeId`), so ids are not stable across a save that creates the
 * workspace — indices are.
 */
interface SavedTileRow {
  weight: number;
  items: { index: number; weight: number }[];
}

/**
 * Parse a workspace's persisted `nodes_json`. It is stored as an object
 * `{ nodes, edges }`, but a legacy format stored a bare array — and a corrupt
 * blob must degrade gracefully rather than throw. Single source for every reader
 * so the two shapes can't be mis-handled again.
 */
export function parseSavedNodes(
  nodesJson: string | null | undefined,
): { nodes: SavedNode[]; edges: SavedEdge[]; tiles: SavedTileRow[] } {
  if (!nodesJson) return { nodes: [], edges: [], tiles: [] };
  try {
    const raw = JSON.parse(nodesJson);
    if (Array.isArray(raw)) return { nodes: raw, edges: [], tiles: [] };
    return { nodes: raw.nodes ?? [], edges: raw.edges ?? [], tiles: raw.tiles ?? [] };
  } catch {
    return { nodes: [], edges: [], tiles: [] };
  }
}

/** Tile layout → index form, for persistence. */
function serializeTiles(
  layout: TileLayout | null,
  indexOf: (nodeId: string) => number,
): SavedTileRow[] {
  if (!layout) return [];
  return layout.rows
    .map((row) => ({
      weight: row.weight,
      items: row.items
        .map((it) => ({ index: indexOf(it.id), weight: it.weight }))
        .filter((it) => it.index >= 0),
    }))
    .filter((row) => row.items.length > 0);
}

/** Index form → tile layout, against the node ids a restore just produced. */
function deserializeTiles(rows: SavedTileRow[], ids: string[]): TileLayout | null {
  if (!Array.isArray(rows) || rows.length === 0) return null;
  const layout: TileLayout = {
    rows: rows
      .map((row) => ({
        weight: typeof row.weight === "number" ? row.weight : 1,
        items: (row.items ?? [])
          .filter((it) => it && it.index >= 0 && it.index < ids.length)
          .map((it) => ({
            id: ids[it.index],
            weight: typeof it.weight === "number" ? it.weight : 1,
          })),
      }))
      .filter((row) => row.items.length > 0),
  };
  return layout.rows.length > 0 ? layout : null;
}

export const WORKSPACE_COLORS = [
  "#3b82f6",
  "#22c55e",
  "#eab308",
  "#ef4444",
  "#a855f7",
  "#06b6d4",
  "#f97316",
  "#64748b",
];

interface WorkspaceState {
  workspaces: Workspace[];
  activeId: string | null;
  load: () => Promise<void>;
  save: (
    name: string,
    viewport: Viewport,
    id?: string,
    color?: string,
    icon?: string,
    colorMode?: string,
  ) => Promise<Workspace>;
  /** Update only metadata (name/color/icon/colorMode) without overwriting saved layout. */
  updateMeta: (
    id: string,
    patch: { name?: string; color?: string; icon?: string; colorMode?: string },
  ) => Promise<void>;
  /** Set (or clear) the workspace's project location for agent context. */
  setProject: (id: string, project: WorkspaceProject | null) => Promise<void>;
  /** Create a new empty workspace, make it active, and clear the canvas. */
  createNew: (name: string) => Promise<void>;
  /** Clear the active selection (no workspace open). */
  deselect: () => void;
  remove: (id: string) => Promise<void>;
  /** Returns the layout + viewport to apply; node reconstruction is done by the canvas. */
  restore: (
    id: string,
  ) => Promise<{
    nodes: CanvasNode[];
    edges: CanvasEdge[];
    viewport: Viewport;
    layout: LayoutMode;
    /** The saved tile arrangement, already keyed to the restored node ids. */
    tiles: TileLayout | null;
  } | null>;
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  workspaces: [],
  activeId: null,

  load: async () => {
    const workspaces = await api.listWorkspaces();
    set({ workspaces });
  },

  save: async (name, viewport, id, color, icon, colorMode) => {
    const { nodes, edges, layoutMode, tileLayout } = useCanvasStore.getState();
    const existing = id ? get().workspaces.find((w) => w.id === id) : undefined;
    const saved: SavedNode[] = nodes.map((n) => {
      const base: SavedNode = {
        id: n.id,
        vpsId: n.data.vpsId,
        name: n.data.name,
        host: n.data.host,
        x: n.position.x,
        y: n.position.y,
        width: (n.width as number) ?? 460,
        height: (n.height as number) ?? 320,
        nodeType: n.type === "sftp" ? "sftp" : "terminal",
      };
      if (n.type === "sftp" && n.data.linkedTerminalId) {
        const idx = nodes.findIndex((x) => x.id === n.data.linkedTerminalId);
        if (idx >= 0) {
          base.linkedTerminalIndex = idx;
          base.followTerminal = n.data.followTerminal ?? true;
        }
      }
      return base;
    });
    const indexOf = (nodeId: string) => nodes.findIndex((n) => n.id === nodeId);
    const savedEdges: SavedEdge[] = edges
      .map((e) => ({
        sourceIndex: indexOf(e.source),
        targetIndex: indexOf(e.target),
      }))
      .filter((e) => e.sourceIndex >= 0 && e.targetIndex >= 0);
    // Reconcile first so a layout that predates the current node set is stored whole
    // rather than half-empty.
    const savedTiles = serializeTiles(
      reconcile(tileLayout, nodes.map((n) => n.id)),
      indexOf,
    );
    const ws = await api.saveWorkspace({
      id,
      name,
      viewport_json: JSON.stringify(viewport),
      layout_mode: layoutMode,
      nodes_json: JSON.stringify({ nodes: saved, edges: savedEdges, tiles: savedTiles }),
      color: color ?? existing?.color ?? null,
      icon: icon ?? existing?.icon ?? null,
      color_mode: colorMode ?? existing?.color_mode ?? null,
      project_json: existing?.project_json ?? null,
    });

    // Rebind the live canvas nodes to the deterministic ids this workspace will
    // use on every future restore, and migrate their session-store entries so the
    // running sessions keep matching (otherwise the first switch-back would miss).
    const sess = useSessionStore.getState();
    const rebound = nodes.map((n, i) => {
      const newId = workspaceNodeId(ws.id, i);
      if (n.id !== newId) {
        const info = sess.sessions[n.id];
        if (info) {
          sess.setInfo(newId, info);
          sess.remove(n.id);
        }
      }
      return { ...n, id: newId };
    });
    const reboundEdges: CanvasEdge[] = edges.map((e) => {
      const srcIdx = nodes.findIndex((n) => n.id === e.source);
      const tgtIdx = nodes.findIndex((n) => n.id === e.target);
      const srcId = srcIdx >= 0 ? workspaceNodeId(ws.id, srcIdx) : e.source;
      const tgtId = tgtIdx >= 0 ? workspaceNodeId(ws.id, tgtIdx) : e.target;
      return {
        ...e,
        id: `link-${srcId}-${tgtId}`,
        source: srcId,
        target: tgtId,
      };
    });
    useCanvasStore.getState().setNodes(rebound);
    useCanvasStore.getState().setEdges(reboundEdges);
    // The nodes just changed id, so the in-memory tile layout would point at ids that
    // no longer exist and silently reset to the balanced default. Re-point it.
    useCanvasStore
      .getState()
      .setTileLayout(deserializeTiles(savedTiles, rebound.map((n) => n.id)));

    await get().load();
    set({ activeId: ws.id });
    return ws;
  },

  updateMeta: async (id, patch) => {
    const w = get().workspaces.find((x) => x.id === id);
    if (!w) return;
    await api.saveWorkspace({
      id: w.id,
      name: patch.name ?? w.name,
      viewport_json: w.viewport_json ?? null,
      layout_mode: w.layout_mode ?? null,
      nodes_json: w.nodes_json ?? null,
      color: patch.color ?? w.color ?? null,
      icon: patch.icon ?? w.icon ?? null,
      color_mode: patch.colorMode ?? w.color_mode ?? null,
      project_json: w.project_json ?? null,
    });
    await get().load();
  },

  setProject: async (id, project) => {
    const w = get().workspaces.find((x) => x.id === id);
    if (!w) return;
    await api.saveWorkspace({
      id: w.id,
      name: w.name,
      viewport_json: w.viewport_json ?? null,
      layout_mode: w.layout_mode ?? null,
      nodes_json: w.nodes_json ?? null,
      color: w.color ?? null,
      icon: w.icon ?? null,
      color_mode: w.color_mode ?? null,
      project_json: project ? JSON.stringify(project) : null,
    });
    await get().load();
  },

  createNew: async (name) => {
    const ws = await api.saveWorkspace({
      name,
      viewport_json: JSON.stringify(defaultViewport),
      layout_mode: "freeform",
      nodes_json: JSON.stringify({ nodes: [], edges: [] }),
    });
    // Start from an empty canvas (background sessions are untouched).
    useCanvasStore.getState().setNodes([]);
    useCanvasStore.getState().setEdges([]);
    await get().load();
    set({ activeId: ws.id });
  },

  deselect: () => set({ activeId: null }),

  remove: async (id) => {
    await api.deleteWorkspace(id);
    if (get().activeId === id) set({ activeId: null });
    await get().load();
  },

  restore: async (id) => {
    const ws = get().workspaces.find((w) => w.id === id);
    if (!ws) return null;
    const { nodes: saved, edges: savedEdges, tiles: savedTiles } = parseSavedNodes(
      ws.nodes_json,
    );
    let viewport: Viewport = { x: 0, y: 0, zoom: 1 };
    if (ws.viewport_json) {
      try {
        viewport = JSON.parse(ws.viewport_json);
      } catch {
        // corrupt viewport blob → keep the default
      }
    }
    const layout = (ws.layout_mode as LayoutMode) || "freeform";
    const nodes: CanvasNode[] = saved.map((s, i) => {
      const nodeId = workspaceNodeId(id, i);
      const data = {
        vpsId: s.vpsId,
        name: s.name,
        host: s.host,
        ...(s.nodeType === "sftp" && s.linkedTerminalIndex != null
          ? {
              linkedTerminalId: workspaceNodeId(id, s.linkedTerminalIndex),
              followTerminal: s.followTerminal ?? true,
            }
          : {}),
      };
      return {
        id: nodeId,
        type: s.nodeType === "sftp" ? "sftp" : "terminal",
        position: { x: s.x, y: s.y },
        width: s.width,
        height: s.height,
        data,
      } as CanvasNode;
    });
    const edges: CanvasEdge[] = savedEdges.map((e) => {
      const srcId = workspaceNodeId(id, e.sourceIndex);
      const tgtId = workspaceNodeId(id, e.targetIndex);
      return {
        id: `link-${srcId}-${tgtId}`,
        source: srcId,
        target: tgtId,
        type: "floating",
        animated: true,
        style: { stroke: "#22d3ee", strokeWidth: 2 },
        data: { kind: "sftp-terminal" },
      };
    });
    const tiles = deserializeTiles(savedTiles, nodes.map((n) => n.id));
    set({ activeId: id });
    return { nodes, edges, viewport, layout, tiles };
  },
}));
