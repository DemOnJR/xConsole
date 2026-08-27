import { type Connection, type Edge, type EdgeChange, type Node, type NodeChange, type Viewport } from "@xyflow/react";
import type { Vps } from "../lib/tauri";
import { type TileLayout } from "../lib/tileLayout";
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
/** The AI agent window node. Binds to the global agent session (Phase 1); a
 *  per-node `sessionId` becomes the binding once multi-agent lands. */
export interface AgentData {
    name: string;
    sessionId?: string;
    [key: string]: unknown;
}
/** A kanban board node for a /goal session (one node per active goal). */
export interface GoalData {
    goalId: string;
    name: string;
    [key: string]: unknown;
}
/** A live HTML/design sandbox preview node. */
export interface PreviewData {
    title: string;
    html: string;
    [key: string]: unknown;
}
export type CanvasEdge = Edge<{
    kind: "sftp-terminal";
}>;
export type TermNode = Node<TermData, "terminal">;
export type SftpNode = Node<SftpData, "sftp">;
export type DbNode = Node<DbData, "db">;
export type AgentNode = Node<AgentData, "agent">;
export type GoalNode = Node<GoalData, "goal">;
export type PreviewNode = Node<PreviewData, "preview">;
export type CanvasNode = TermNode | SftpNode | DbNode | AgentNode | GoalNode | PreviewNode;
export declare const NODE_W = 460;
export declare const NODE_H = 320;
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
    paneSize: {
        width: number;
        height: number;
    } | null;
    /** Pending terminal commands from the agent chat Execute button (FIFO per node). */
    pendingTerminalCommands: Record<string, {
        command: string;
        send: boolean;
    }[]>;
    setNodes: (nodes: CanvasNode[]) => void;
    setEdges: (edges: CanvasEdge[]) => void;
    onNodesChange: (changes: NodeChange<CanvasNode>[]) => void;
    onEdgesChange: (changes: EdgeChange<CanvasEdge>[]) => void;
    onConnect: (connection: Connection) => void;
    updateNodeData: (id: string, partial: Partial<SftpData & PreviewData>) => void;
    addVps: (vps: Vps, position?: {
        x: number;
        y: number;
    }) => string;
    addSftp: (vps: Vps, position?: {
        x: number;
        y: number;
    }) => string;
    /** Drop a database browser for this server onto the canvas. */
    addDb: (vps: Vps, position?: {
        x: number;
        y: number;
    }) => string;
    /** Open the agent window (single instance — focuses it if one is open). */
    addAgent: (position?: {
        x: number;
        y: number;
    }) => string;
    /** Toggle the agent window (opens if closed, closes/removes if open). */
    toggleAgent: (position?: {
        x: number;
        y: number;
    }) => string | null;
    /** Open a kanban board node for a goal session (multiple allowed). */
    addGoal: (goalId: string, position?: {
        x: number;
        y: number;
    }) => string;
    /** Open a live HTML/design sandbox preview node on the canvas. */
    addPreview: (opts: {
        id?: string;
        title: string;
        html: string;
        width?: number;
        height?: number;
        position?: {
            x: number;
            y: number;
        };
    }) => string;
    removeNode: (id: string) => void;
    setLayout: (mode: LayoutMode) => void;
    focus: (id: string | null) => void;
    isWebgl: (id: string) => boolean;
    /** Arrange nodes into the current tile layout. With `dims` (the canvas pane size in
     *  px) every node is resized so the rows fill the window edge-to-edge. */
    arrangeTiles: (dims?: {
        width: number;
        height: number;
    }) => void;
    /** Record the live pane size so layout edits can re-tile on their own. */
    setPaneSize: (dims: {
        width: number;
        height: number;
    }) => void;
    /**
     * Re-tile, taking the arrangement from where the nodes currently sit. This is what
     * the Tile button does: drag terminals roughly into place, press it, and the grid
     * adopts that shape (three side by side become one row of three).
     */
    retileFromPositions: (dims?: {
        width: number;
        height: number;
    }) => void;
    /** Install an arrangement wholesale (workspace restore, or after an id rebind). */
    setTileLayout: (layout: TileLayout | null) => void;
    /** Re-flow into rows of the given sizes, e.g. `[3, 2]` for 3 on top, 2 below. */
    setTileRows: (counts: number[]) => void;
    /** Re-flow into side-by-side columns, e.g. `[2, 1]` for 2 stacked left, 1 right. */
    setTileColumns: (counts: number[]) => void;
    /** Discard any hand-tuned arrangement and go back to the balanced default. */
    resetTileLayout: () => void;
    /** Move a tile within its row (`horizontal`) or between rows (`vertical`). */
    moveTile: (id: string, dir: -1 | 1, axis: TileMoveAxis) => void;
    /** Grow/shrink a tile's width share, or its row's height share. */
    growTile: (id: string, delta: number, axis: TileMoveAxis) => void;
    /** Give a tile its own full-width row — or merge it back. */
    toggleTileFullWidth: (id: string) => void;
    toggleFillPane: (id: string) => void;
    clear: () => void;
    /**
     * Queue a command to be typed into a terminal node once its SSH session is ready.
     * `send=true` runs it (appends newline); `send=false` types it and waits (the user
     * presses Enter). Used by the agent chat's Execute button.
     */
    queueTerminalCommand: (nodeId: string, command: string, send: boolean) => void;
    /** Take (and clear) the queued command for a node — called by TerminalNode. */
    takeTerminalCommand: (nodeId: string) => {
        command: string;
        send: boolean;
    } | null;
}
export declare const useCanvasStore: import("zustand").UseBoundStore<Omit<import("zustand").StoreApi<CanvasState>, "setState" | "persist"> & {
    setState(partial: CanvasState | Partial<CanvasState> | ((state: CanvasState) => CanvasState | Partial<CanvasState>), replace?: false | undefined): unknown;
    setState(state: CanvasState | ((state: CanvasState) => CanvasState), replace: true): unknown;
    persist: {
        setOptions: (options: Partial<import("zustand/middleware").PersistOptions<CanvasState, unknown, unknown>>) => void;
        clearStorage: () => void;
        rehydrate: () => Promise<void> | void;
        hasHydrated: () => boolean;
        onHydrate: (fn: (state: CanvasState) => void) => () => void;
        onFinishHydration: (fn: (state: CanvasState) => void) => () => void;
        getOptions: () => Partial<import("zustand/middleware").PersistOptions<CanvasState, unknown, unknown>>;
    };
}>;
export declare const defaultViewport: Viewport;
export {};
//# sourceMappingURL=canvasStore.d.ts.map