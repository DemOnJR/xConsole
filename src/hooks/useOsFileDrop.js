import { useEffect } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
/** Registered by whoever cares about files being dropped on a `data-drop` target. */
const handlers = new Set();
/** The target currently under a dragged file, for highlighting. */
let hoverTarget = null;
const hoverListeners = new Set();
function setHover(t) {
    if (t === hoverTarget)
        return;
    hoverTarget = t;
    hoverListeners.forEach((f) => f(t));
}
export function onOsFilesDropped(fn) {
    handlers.add(fn);
    return () => handlers.delete(fn);
}
export function onOsDropHover(fn) {
    hoverListeners.add(fn);
    return () => hoverListeners.delete(fn);
}
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
export function useOsFileDrop() {
    useEffect(() => {
        let un;
        void getCurrentWebview()
            .onDragDropEvent((event) => {
            const p = event.payload;
            const dpr = window.devicePixelRatio || 1;
            const at = (pos) => ({
                x: pos.x / dpr,
                y: pos.y / dpr,
            });
            if (p.type === "over") {
                const { x, y } = at(p.position);
                const el = document.elementFromPoint(x, y);
                setHover(el?.closest("[data-drop]")?.dataset.drop ?? null);
                return;
            }
            if (p.type === "leave") {
                setHover(null);
                return;
            }
            if (p.type === "drop") {
                const { x, y } = at(p.position);
                const el = document.elementFromPoint(x, y);
                const target = el?.closest("[data-drop]")?.dataset.drop ?? null;
                setHover(null);
                if (!target || !p.paths?.length)
                    return;
                handlers.forEach((fn) => fn(target, p.paths));
            }
        })
            .then((f) => (un = f));
        return () => un?.();
    }, []);
}
//# sourceMappingURL=useOsFileDrop.js.map