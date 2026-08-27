import type { Viewport } from "@xyflow/react";
import { type Workspace, type WorkspaceProject } from "../lib/tauri";
import { type CanvasEdge, type CanvasNode, type LayoutMode } from "./canvasStore";
import { type TileLayout } from "../lib/tileLayout";
import { type SavedSplit } from "../lib/tileTree";
/** Deterministic node id for a workspace slot (stable across reopen). */
export declare const workspaceNodeId: (workspaceId: string, index: number) => string;
/** True when every live node already uses the deterministic workspace id. */
export declare function workspaceIdsAlreadyBound(workspaceId: string, nodeIds: string[]): boolean;
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
    items: {
        index: number;
        weight: number;
    }[];
}
/** A persisted column pane (side-by-side layout). */
interface SavedTileColumn {
    weight: number;
    items: {
        index: number;
        weight: number;
    }[];
}
/**
 * Parse a workspace's persisted `nodes_json`. It is stored as an object
 * `{ nodes, edges }`, but a legacy format stored a bare array — and a corrupt
 * blob must degrade gracefully rather than throw. Single source for every reader
 * so the two shapes can't be mis-handled again.
 */
export declare function parseSavedNodes(nodesJson: string | null | undefined): {
    nodes: SavedNode[];
    edges: SavedEdge[];
    tiles: SavedTileRow[];
    columns?: SavedTileColumn[];
    tree?: SavedSplit;
};
/** Index form → tile layout, against the node ids a restore just produced. */
export declare function deserializeTiles(rows: SavedTileRow[], columns: SavedTileColumn[] | undefined, ids: string[], tree?: SavedSplit): TileLayout | null;
export declare const WORKSPACE_COLORS: string[];
interface WorkspaceState {
    workspaces: Workspace[];
    activeId: string | null;
    load: () => Promise<void>;
    save: (name: string, viewport: Viewport, id?: string, color?: string, icon?: string, colorMode?: string) => Promise<Workspace>;
    /** Update only metadata (name/color/icon/colorMode) without overwriting saved layout. */
    updateMeta: (id: string, patch: {
        name?: string;
        color?: string;
        icon?: string;
        colorMode?: string;
    }) => Promise<void>;
    /** Set (or clear) the workspace's project location for agent context. */
    setProject: (id: string, project: WorkspaceProject | null) => Promise<void>;
    /** Create a new empty workspace, make it active, and clear the canvas. */
    createNew: (name: string) => Promise<void>;
    /** Clear the active selection (no workspace open). */
    deselect: () => void;
    remove: (id: string) => Promise<void>;
    /** Returns the layout + viewport to apply; node reconstruction is done by the canvas. */
    restore: (id: string) => Promise<{
        nodes: CanvasNode[];
        edges: CanvasEdge[];
        viewport: Viewport;
        layout: LayoutMode;
        /** The saved tile arrangement, already keyed to the restored node ids. */
        tiles: TileLayout | null;
    } | null>;
}
export declare const useWorkspaceStore: import("zustand").UseBoundStore<import("zustand").StoreApi<WorkspaceState>>;
export {};
//# sourceMappingURL=workspaceStore.d.ts.map