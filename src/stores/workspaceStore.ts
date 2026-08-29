import { create } from "zustand";
import type { Viewport } from "@xyflow/react";
import { api, type Workspace, type WorkspaceProject } from "../lib/tauri";
import { stableViewport } from "../lib/workspacePersist";
import {
  defaultViewport,
  useCanvasStore,
  type CanvasEdge,
  type CanvasNode,
  type LayoutMode,
} from "./canvasStore";
import { reconcile, type TileLayout } from "../lib/tileLayout";
import {
  deserializeSplit,
  serializeSplit,
  treeOf,
  type SavedSplit,
} from "../lib/tileTree";

/**
 * Fallback node id for a workspace slot, for saves that predate stored ids.
 *
 * Positional, and that is the whole problem with it: closing a node shifts every node
 * after it into a new slot. Only [`restoredNodeIds`] may use it, and only where there
 * is no stored id to prefer.
 */
export const workspaceNodeId = (workspaceId: string, index: number) =>
  `${workspaceId}::${index}`;

/**
 * The id each saved node is restored under.
 *
 * A node id is an identity, not a position. It keys the live SSH session
 * (`sessionStore.sessions`) and it is React's key for the pane, so re-deriving it from
 * the array index meant that closing one terminal renamed every terminal after it —
 * and because the old and new id sets overlapped, React reused a pane component under
 * an id that now belonged to a *different* session. The reused pane kept streaming the
 * session it was already attached to (its effect watches `[id, vpsId]`, and for two
 * terminals on the same host neither changed), while a freshly mounted pane picked up
 * that same session from the store. Two panes, one session, and the third left running
 * on the server with nothing showing it.
 *
 * So: the stored id wins, always. The slot is a fallback for saves written before ids
 * were stored, and a tiebreak for a corrupt file with duplicates — never a rename of a
 * node that already has a name.
 */
export function restoredNodeIds(workspaceId: string, saved: SavedNode[]): string[] {
  const ids: (string | null)[] = saved.map(() => null);
  const used = new Set<string>();

  // Stored ids are claimed first, across the whole set. Interleaving the two passes
  // would let a slot fallback squat on an id a later node was about to claim as its
  // own — renaming a live node, which is the failure this function exists to prevent.
  // A repeat is left for the second pass: two nodes sharing an id is exactly the state
  // that mirrors one terminal into two panes.
  saved.forEach((n, i) => {
    const stored = n.id?.trim();
    if (stored && !used.has(stored)) {
      ids[i] = stored;
      used.add(stored);
    }
  });

  // Whatever is left gets a free slot. Probing past the end of the array keeps the
  // fallbacks clear of any stored id that already looks like a slot.
  let probe = 0;
  return ids.map((id, i) => {
    if (id) return id;
    let slot = workspaceNodeId(workspaceId, i);
    while (used.has(slot)) slot = workspaceNodeId(workspaceId, saved.length + probe++);
    used.add(slot);
    return slot;
  });
}

/** Serialized node persisted in a workspace (no live session state). */
export interface SavedNode {
  /**
   * The node's identity, and the key its live SSH session is stored under.
   *
   * Absent only in saves old enough to predate it being written, which restore fills
   * in by slot. Everything else must treat it as the node's name: a node that is
   * still on screen never gets a different one.
   */
  id?: string;
  vpsId: string;
  name: string;
  host: string;
  x: number;
  y: number;
  width: number;
  height: number;
  nodeType?: "terminal" | "sftp" | "db" | "agent" | "goal";
  linkedTerminalIndex?: number;
  followTerminal?: boolean;
  /** Persisted /goal session id (kanban boards). */
  goalId?: string;
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

/** A persisted column pane (side-by-side layout). */
interface SavedTileColumn {
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
): {
  nodes: SavedNode[];
  edges: SavedEdge[];
  tiles: SavedTileRow[];
  columns?: SavedTileColumn[];
  tree?: SavedSplit;
} {
  if (!nodesJson) return { nodes: [], edges: [], tiles: [] };
  try {
    const raw = JSON.parse(nodesJson);
    if (Array.isArray(raw)) return { nodes: raw, edges: [], tiles: [] };
    return {
      nodes: raw.nodes ?? [],
      edges: raw.edges ?? [],
      tiles: raw.tiles ?? [],
      columns: raw.columns,
      tree: raw.tree,
    };
  } catch {
    return { nodes: [], edges: [], tiles: [] };
  }
}

/** Tile layout → index form, for persistence. Persists BOTH rows and columns so a
 *  side-by-side arrangement survives a save/restore (monitor move → autosave →
 *  reload would otherwise flatten columns into one long row). */
function serializeTiles(
  layout: TileLayout | null,
  indexOf: (nodeId: string) => number,
): { rows: SavedTileRow[]; columns?: SavedTileColumn[]; tree?: SavedSplit } {
  if (!layout) return { rows: [] };
  const toSaved = (items: { id: string; weight: number }[]) =>
    items
      .map((it) => ({ index: indexOf(it.id), weight: it.weight }))
      .filter((it) => it.index >= 0);
  const tree = serializeSplit(layout.tree ?? treeOf(layout), indexOf);
  return {
    rows: layout.rows
      .map((row) => ({ weight: row.weight, items: toSaved(row.items) }))
      .filter((row) => row.items.length > 0),
    columns: layout.columns
      ?.map((col) => ({ weight: col.weight, items: toSaved(col.items) }))
      .filter((col) => col.items.length > 0),
    tree: tree ?? undefined,
  };
}

/** Index form → tile layout, against the node ids a restore just produced. */
export function deserializeTiles(
  rows: SavedTileRow[],
  columns: SavedTileColumn[] | undefined,
  ids: string[],
  tree?: SavedSplit,
): TileLayout | null {
  const toItems = (items: { index: number; weight: number }[]) =>
    items
      .filter((it) => it && it.index >= 0 && it.index < ids.length)
      .map((it) => ({
        id: ids[it.index],
        weight: typeof it.weight === "number" ? it.weight : 1,
      }))
      .filter((it) => ids.includes(it.id));

  // Column layout persisted → rebuild columns and mirror rows from them (same as
  // layoutFromColumnCounts) so every op stays consistent.
  if (columns && columns.length > 0) {
    const cols = columns
      .map((col) => ({ weight: col.weight ?? 1, items: toItems(col.items) }))
      .filter((col) => col.items.length > 0);
    if (cols.length > 0) {
      const flat = cols.flatMap((c) => c.items);
      const layout: TileLayout = {
        rows: flat.length > 0 ? [{ weight: 1, items: flat }] : [],
        columns: cols,
      };
      // Columns are a flat fallback (one stack per x-band). A nested left
      // pane — 1 over 2 beside a full-height right — lives only on `tree`.
      // Dropping it here made refresh collapse that layout to 3 stacked left.
      if (tree) {
        const parsed = deserializeSplit(tree, ids);
        if (parsed) layout.tree = parsed;
      }
      return layout;
    }
  }

  if (!Array.isArray(rows) || rows.length === 0) {
    if (tree) {
      const parsed = deserializeSplit(tree, ids);
      if (parsed) return { rows: [{ weight: 1, items: [] }], tree: parsed };
    }
    return null;
  }
  const layout: TileLayout = {
    rows: rows
      .map((row) => ({
        weight: typeof row.weight === "number" ? row.weight : 1,
        items: toItems(row.items),
      }))
      .filter((row) => row.items.length > 0),
  };
  if (layout.rows.length === 0) return null;
  if (tree) {
    const parsed = deserializeSplit(tree, ids);
    if (parsed) layout.tree = parsed;
  }
  return layout;
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
        vpsId: String(n.data.vpsId ?? ""),
        name: String(n.data.name ?? ""),
        host: String(n.data.host ?? ""),
        x: n.position.x,
        y: n.position.y,
        width: (n.width as number) ?? 460,
        height: (n.height as number) ?? 320,
        // Explicit per type, not a two-way ternary: the old
        // `n.type === "sftp" ? "sftp" : "terminal"` silently turned any third node type
        // into a terminal on save, losing it on the next restore.
        nodeType:
          n.type === "sftp" ? "sftp" : n.type === "db" ? "db" : n.type === "agent" ? "agent" : n.type === "goal" ? "goal" : "terminal",
      };
      if (n.type === "sftp" && n.data.linkedTerminalId) {
        const idx = nodes.findIndex((x) => x.id === n.data.linkedTerminalId);
        if (idx >= 0) {
          base.linkedTerminalIndex = idx;
          base.followTerminal = n.data.followTerminal ?? true;
        }
      }
      if (n.type === "goal" && n.data.goalId) {
        base.goalId = String(n.data.goalId);
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
      viewport_json: JSON.stringify(stableViewport(viewport)),
      layout_mode: layoutMode,
      nodes_json: JSON.stringify({
        nodes: saved,
        edges: savedEdges,
        tiles: savedTiles.rows,
        columns: savedTiles.columns,
        tree: savedTiles.tree,
      }),
      color: color ?? existing?.color ?? null,
      icon: icon ?? existing?.icon ?? null,
      color_mode: colorMode ?? existing?.color_mode ?? null,
      project_json: existing?.project_json ?? null,
    });

    // Nothing is re-keyed here on purpose. Node ids are written to the save as they
    // are and read back as they are; a live node keeps the id its session is stored
    // under for as long as it is on screen. See `restoredNodeIds`.

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
    const {
      nodes: saved,
      edges: savedEdges,
      tiles: savedTiles,
      columns: savedColumns,
      tree: savedTree,
    } = parseSavedNodes(ws.nodes_json);
    let viewport: Viewport = { x: 0, y: 0, zoom: 1 };
    if (ws.viewport_json) {
      try {
        viewport = JSON.parse(ws.viewport_json);
      } catch {
        // corrupt viewport blob → keep the default
      }
    }
    const layout = (ws.layout_mode as LayoutMode) || "freeform";
    // Worked out for the whole set first, so the links and edges below resolve a slot
    // to the id that slot is actually being restored under.
    const ids = restoredNodeIds(id, saved);
    const idAt = (index: number) => ids[index] ?? workspaceNodeId(id, index);
    const nodes: CanvasNode[] = saved.map((s, i) => {
      const nodeId = ids[i];
      const data = {
        vpsId: s.vpsId,
        name: s.name,
        host: s.host,
        ...(s.nodeType === "sftp" && s.linkedTerminalIndex != null
          ? {
              linkedTerminalId: idAt(s.linkedTerminalIndex),
              followTerminal: s.followTerminal ?? true,
            }
          : {}),
        ...(s.nodeType === "goal" && s.goalId ? { goalId: s.goalId } : {}),
      };
      return {
        id: nodeId,
        type:
          s.nodeType === "sftp"
            ? "sftp"
            : s.nodeType === "db"
              ? "db"
              : s.nodeType === "agent"
                ? "agent"
                : s.nodeType === "goal"
                  ? "goal"
                  : "terminal",
        position: { x: s.x, y: s.y },
        width: s.width,
        height: s.height,
        data,
      } as CanvasNode;
    });
    const edges: CanvasEdge[] = savedEdges.map((e) => {
      const srcId = idAt(e.sourceIndex);
      const tgtId = idAt(e.targetIndex);
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
    const tiles = deserializeTiles(savedTiles, savedColumns, nodes.map((n) => n.id), savedTree);
    set({ activeId: id });
    return { nodes, edges, viewport, layout, tiles };
  },
}));
