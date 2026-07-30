import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Browser-style back/forward history for anything you navigate through — an SFTP
 * directory, a database table — including the mouse's side buttons.
 *
 * The model is a back stack, a current entry, and a forward stack. Visiting somewhere new
 * pushes the current entry onto the back stack and **clears forward**, which is what makes
 * it behave like a browser rather than a ring buffer: going back twice then somewhere new
 * shouldn't leave a stale "forward" pointing at a branch you abandoned.
 *
 * `go` is only called for back/forward moves, never for `visit`, so the caller can use one
 * function for "the user clicked something" without re-entering.
 */
export interface NavHistory<T> {
  /** Record a move the user just made. */
  visit: (entry: T) => void;
  back: () => void;
  forward: () => void;
  canBack: boolean;
  canForward: boolean;
  /** Replace the current entry without touching the stacks (e.g. a refresh). */
  replace: (entry: T) => void;
}

export function useNavHistory<T>({
  current,
  go,
  isSame = (a: T, b: T) => a === b,
}: {
  /** Where we are now — used as the entry pushed onto the back stack. */
  current: T | null;
  /** Apply a history entry. Called only by back/forward. */
  go: (entry: T) => void;
  /** Equality, so re-selecting the same place doesn't create a dead history step. */
  isSame?: (a: T, b: T) => boolean;
}): NavHistory<T> {
  const back$ = useRef<T[]>([]);
  const forward$ = useRef<T[]>([]);
  const current$ = useRef<T | null>(current);
  // Mirrors the ref depths so the UI can disable the buttons; the refs themselves are
  // what the handlers read, to avoid stale closures in the window listener.
  const [depths, setDepths] = useState({ back: 0, forward: 0 });

  useEffect(() => {
    current$.current = current;
  }, [current]);

  const sync = useCallback(() => {
    setDepths({ back: back$.current.length, forward: forward$.current.length });
  }, []);

  const visit = useCallback(
    (entry: T) => {
      const now = current$.current;
      if (now != null && isSame(now, entry)) return;
      if (now != null) back$.current.push(now);
      forward$.current = [];
      current$.current = entry;
      sync();
    },
    [isSame, sync],
  );

  const replace = useCallback((entry: T) => {
    current$.current = entry;
  }, []);

  const back = useCallback(() => {
    const prev = back$.current.pop();
    if (prev === undefined) return;
    const now = current$.current;
    if (now != null) forward$.current.push(now);
    current$.current = prev;
    sync();
    go(prev);
  }, [go, sync]);

  const forward = useCallback(() => {
    const next = forward$.current.pop();
    if (next === undefined) return;
    const now = current$.current;
    if (now != null) back$.current.push(now);
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
export function useMouseNavButtons(
  ref: React.RefObject<HTMLElement | null>,
  history: Pick<NavHistory<unknown>, "back" | "forward">,
) {
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      // 3 = back, 4 = forward. Anything else is a normal click.
      if (e.button !== 3 && e.button !== 4) return;
      const el = ref.current;
      if (!el || !(e.target instanceof Node) || !el.contains(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.button === 3) history.back();
      else history.forward();
    };
    window.addEventListener("mousedown", onDown, true);
    return () => window.removeEventListener("mousedown", onDown, true);
  }, [ref, history]);
}
