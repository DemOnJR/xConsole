/**
 * The rules a file manager's selection has to get right.
 *
 * Both of these are the kind of logic that looks obvious, is quietly wrong in one
 * direction, and only shows up when the wrong six files have already been deleted — so
 * they live here as pure functions with tests rather than inline in the component.
 */
/**
 * Which paths an action applies to, given the row it was invoked on.
 *
 * Right-clicking a row *outside* the selection acts on that row alone, because that is
 * what the click visibly did — the selection moves there. Right-clicking *inside* the
 * selection acts on all of it. Getting this backwards means a menu that silently applies
 * to one of six highlighted rows, or to six when the user pointed at one.
 */
export declare function toggleSelection(selection: ReadonlySet<string>, path: string): Set<string>;
export declare function actionTargets(entry: {
    path: string;
} | null, selection: ReadonlySet<string>): string[];
/**
 * The rows covered by a shift-click, from `anchor` to `path` inclusive.
 *
 * Order-independent: dragging a range upwards selects the same rows as dragging it down.
 * An anchor that is no longer on screen — the listing was filtered or refreshed under it —
 * yields just the clicked row rather than an empty or wildly wrong range.
 */
export declare function rangeBetween(list: readonly string[], anchor: string | null, path: string): string[];
/**
 * Parse the advanced search's extension box.
 *
 * People type these every way there is — `php js`, `.php,.js`, `php, js` — and a leading
 * dot is how an extension is usually written but never how it is stored, so it has to come
 * off before the pattern is built.
 */
export declare function parseExtensions(input: string): string[];
//# sourceMappingURL=selection.d.ts.map