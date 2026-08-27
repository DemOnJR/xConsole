/** Stable persist key for the workspace autosave. Ignores focus, selection, and subpixel jitter. */
export type PersistViewport = {
    x: number;
    y: number;
    zoom: number;
};
export type PersistableCanvas = {
    nodes: Array<{
        id: string;
        type?: string;
        position: {
            x: number;
            y: number;
        };
        width?: number | null;
        height?: number | null;
        data: Record<string, unknown>;
    }>;
    edges: Array<{
        source: string;
        target: string;
    }>;
    layoutMode: string;
    tileLayout: unknown;
};
/** JSON key of the layout that is worth writing to disk. */
export declare function workspacePersistKey(canvas: PersistableCanvas, viewport: PersistViewport): string;
/** Round a live viewport so a 1e-10 pan does not rewrite the workspace row. */
export declare function stableViewport(viewport: PersistViewport): PersistViewport;
//# sourceMappingURL=workspacePersist.d.ts.map