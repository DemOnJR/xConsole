import { useCallback, useEffect, useRef, useState } from "react";
export function useNavHistory({ current, go, isSame = (a, b) => a === b, }) {
    const back$ = useRef([]);
    const forward$ = useRef([]);
    const current$ = useRef(current);
    // Mirrors the ref depths so the UI can disable the buttons; the refs themselves are
    // what the handlers read, to avoid stale closures in the window listener.
    const [depths, setDepths] = useState({ back: 0, forward: 0 });
    useEffect(() => {
        current$.current = current;
    }, [current]);
    const sync = useCallback(() => {
        setDepths({ back: back$.current.length, forward: forward$.current.length });
    }, []);
    const visit = useCallback((entry) => {
        const now = current$.current;
        if (now != null && isSame(now, entry))
            return;
        if (now != null)
            back$.current.push(now);
        forward$.current = [];
        current$.current = entry;
        sync();
    }, [isSame, sync]);
    const replace = useCallback((entry) => {
        current$.current = entry;
    }, []);
    const back = useCallback(() => {
        const prev = back$.current.pop();
        if (prev === undefined)
            return;
        const now = current$.current;
        if (now != null)
            forward$.current.push(now);
        current$.current = prev;
        sync();
        go(prev);
    }, [go, sync]);
    const forward = useCallback(() => {
        const next = forward$.current.pop();
        if (next === undefined)
            return;
        const now = current$.current;
        if (now != null)
            back$.current.push(now);
        current$.current = next;
        sync();
        go(next);
    }, [go, sync]);
    return {
        visit,
        back,
        forward,
        replace,
        canBack: depths.back > 0,
        canForward: depths.forward > 0,
    };
}
/**
 * Wire the mouse's side buttons to a history, but only while the pointer is inside
 * `ref` — otherwise every panel on the canvas would react to one click.
 *
 * Listens on `mousedown` in the **capture** phase and calls `preventDefault`. Buttons 3
 * and 4 are the webview's own back/forward, so without this the whole app would try to
 * navigate away from the page. `auxclick` is too late to stop that.
 */
export function useMouseNavButtons(ref, history) {
    useEffect(() => {
        const onDown = (e) => {
            // 3 = back, 4 = forward. Anything else is a normal click.
            if (e.button !== 3 && e.button !== 4)
                return;
            const el = ref.current;
            if (!el || !(e.target instanceof Node) || !el.contains(e.target))
                return;
            e.preventDefault();
            e.stopPropagation();
            if (e.button === 3)
                history.back();
            else
                history.forward();
        };
        window.addEventListener("mousedown", onDown, true);
        return () => window.removeEventListener("mousedown", onDown, true);
    }, [ref, history]);
}
//# sourceMappingURL=useNavHistory.js.map