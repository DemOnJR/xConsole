type DropHandler = (target: string, paths: string[]) => void;
export declare function onOsFilesDropped(fn: DropHandler): () => void;
export declare function onOsDropHover(fn: (t: string | null) => void): () => void;
/**
 * Files dragged in from Explorer.
 *
 * Tauri reports these as a window-level event with a cursor position rather than a DOM
 * event on an element, so the drop target is found by hit-testing that point. The position
 * arrives in **physical** pixels while `elementFromPoint` wants CSS pixels, which is a
 * silent one-monitor-works-one-doesn't bug on any display that is not at 100% scaling —
 * hence the explicit `devicePixelRatio` division.
 *
 * Mounted once, at app level.
 */
export declare function useOsFileDrop(): void;
export {};
//# sourceMappingURL=useOsFileDrop.d.ts.map