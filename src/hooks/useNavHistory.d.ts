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
export declare function useNavHistory<T>({ current, go, isSame, }: {
    /** Where we are now — used as the entry pushed onto the back stack. */
    current: T | null;
    /** Apply a history entry. Called only by back/forward. */
    go: (entry: T) => void;
    /** Equality, so re-selecting the same place doesn't create a dead history step. */
    isSame?: (a: T, b: T) => boolean;
}): NavHistory<T>;
/**
 * Wire the mouse's side buttons to a history, but only while the pointer is inside
 * `ref` — otherwise every panel on the canvas would react to one click.
 *
 * Listens on `mousedown` in the **capture** phase and calls `preventDefault`. Buttons 3
 * and 4 are the webview's own back/forward, so without this the whole app would try to
 * navigate away from the page. `auxclick` is too late to stop that.
 */
export declare function useMouseNavButtons(ref: React.RefObject<HTMLElement | null>, history: Pick<NavHistory<unknown>, "back" | "forward">): void;
//# sourceMappingURL=useNavHistory.d.ts.map