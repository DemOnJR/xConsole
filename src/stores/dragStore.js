import { create } from "zustand";
import { useSnapDragStore } from "../lib/snapDrag";
export const useDragStore = create((set) => ({
    drag: null,
    x: 0,
    y: 0,
    over: null,
    begin: (drag, x, y) => set({ drag, x, y, over: null }),
    move: (x, y, over) => set({ x, y, over }),
    end: () => set({ drag: null, over: null }),
}));
/** The `data-drop` id under a viewport point, if any. */
export function dropTargetAt(x, y) {
    const el = document.elementFromPoint(x, y);
    const target = el?.closest("[data-drop]");
    return target?.dataset.drop ?? null;
}
export function targetForPayload(target, payload, x, y) {
    if (payload.kind === "vps") {
        if (target && target.startsWith("server-row:"))
            return target;
        const canvas = document.querySelector("[data-drop='canvas']");
        const el = document.elementFromPoint(x, y);
        if (canvas && el && (canvas === el || canvas.contains(el))) {
            return "canvas";
        }
    }
    return target;
}
const dropHandlers = new Map();
/**
 * Handle internal drops on a target.
 *
 * `key` matches a `data-drop` id exactly, or as a prefix when the id is `key:something` —
 * so a list can register once for all its rows while a single node registers for itself.
 */
export function onInternalDrop(key, fn) {
    let set = dropHandlers.get(key);
    if (!set) {
        set = new Set();
        dropHandlers.set(key, set);
    }
    set.add(fn);
    return () => {
        set.delete(fn);
        if (set.size === 0)
            dropHandlers.delete(key);
    };
}
function dispatchDrop(target, payload, x, y) {
    const keys = [target];
    const colon = target.indexOf(":");
    if (colon > 0)
        keys.push(target.slice(0, colon));
    for (const k of keys) {
        dropHandlers.get(k)?.forEach((fn) => fn(payload, x, y, target));
    }
}
/**
 * Start an internal drag from a pointerdown.
 *
 * Nothing happens until the pointer has actually moved a few pixels, so a plain click on a
 * file row still selects it rather than starting a drag nobody asked for.
 */
export function startInternalDrag(e, payload, onDrop, onStart) {
    const startX = e.clientX;
    const startY = e.clientY;
    let armed = false;
    const THRESHOLD = 4;
    const onMove = (ev) => {
        if (!armed) {
            if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < THRESHOLD)
                return;
            armed = true;
            onStart?.();
            useDragStore.getState().begin(payload, ev.clientX, ev.clientY);
        }
        const raw = dropTargetAt(ev.clientX, ev.clientY);
        const target = targetForPayload(raw, payload, ev.clientX, ev.clientY);
        useDragStore.getState().move(ev.clientX, ev.clientY, target);
        if (payload.kind === "vps" && target === "canvas") {
            const pane = document.querySelector(".react-flow__pane")?.getBoundingClientRect();
            if (pane && pane.width > 0 && pane.height > 0) {
                const snapState = useSnapDragStore.getState();
                if (!snapState.nodeId)
                    snapState.begin("__new_vps__");
                snapState.move((ev.clientX - pane.left) / pane.width, (ev.clientY - pane.top) / pane.height);
            }
        }
        else if (payload.kind === "vps" && useSnapDragStore.getState().nodeId === "__new_vps__") {
            useSnapDragStore.getState().end();
        }
    };
    const onUp = (ev) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        if (!armed)
            return;
        const raw = dropTargetAt(ev.clientX, ev.clientY);
        const target = targetForPayload(raw, payload, ev.clientX, ev.clientY);
        useDragStore.getState().end();
        if (target) {
            onDrop?.(target, payload);
            dispatchDrop(target, payload, ev.clientX, ev.clientY);
        }
        if (useSnapDragStore.getState().nodeId === "__new_vps__") {
            useSnapDragStore.getState().end();
        }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
}
//# sourceMappingURL=dragStore.js.map