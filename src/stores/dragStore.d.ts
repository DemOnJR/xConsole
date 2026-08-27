/**
 * Internal drag-and-drop, built on pointer events instead of HTML5 DnD.
 *
 * HTML5 drag events do not fire once Tauri's `dragDropEnabled` is on, because the webview
 * hands native drags to the OS layer so the app can receive dropped *files*. We need both:
 * files dropped in from Explorer, and servers/remote files dragged around inside the app.
 * Pointer events are unaffected by that setting, so internal drags use them and the two
 * mechanisms stop competing.
 *
 * Drop targets are plain DOM: any element carrying `data-drop="<id>"`. On release the
 * element under the cursor is hit-tested with `elementFromPoint`, which means targets need
 * no registration and work inside React Flow's transformed canvas.
 */
export type DragKind = "vps" | "remote-file" | "local-file";
export interface DragPayload {
    kind: DragKind;
    /** VPS id, or the SFTP session's vps id for a remote file. Empty for pure local. */
    vpsId: string;
    /** Primary absolute path (remote-file / local-file). */
    path?: string;
    /** Multi-select paths when dragging a selection (same pane). */
    paths?: string[];
    /** Display label for the ghost. */
    label: string;
    /** Directory rather than file. */
    isDir?: boolean;
}
interface DragState {
    drag: DragPayload | null;
    x: number;
    y: number;
    /** data-drop id currently under the cursor, for highlighting. */
    over: string | null;
    begin: (p: DragPayload, x: number, y: number) => void;
    move: (x: number, y: number, over: string | null) => void;
    end: () => void;
}
export declare const useDragStore: import("zustand").UseBoundStore<import("zustand").StoreApi<DragState>>;
/** The `data-drop` id under a viewport point, if any. */
export declare function dropTargetAt(x: number, y: number): string | null;
export declare function targetForPayload(target: string | null, payload: DragPayload, x: number, y: number): string | null;
type DropFn = (payload: DragPayload, x: number, y: number, target: string) => void;
/**
 * Handle internal drops on a target.
 *
 * `key` matches a `data-drop` id exactly, or as a prefix when the id is `key:something` —
 * so a list can register once for all its rows while a single node registers for itself.
 */
export declare function onInternalDrop(key: string, fn: DropFn): () => void;
/**
 * Start an internal drag from a pointerdown.
 *
 * Nothing happens until the pointer has actually moved a few pixels, so a plain click on a
 * file row still selects it rather than starting a drag nobody asked for.
 */
export declare function startInternalDrag(e: React.PointerEvent, payload: DragPayload, onDrop?: (target: string, payload: DragPayload) => void, onStart?: () => void): void;
export {};
//# sourceMappingURL=dragStore.d.ts.map