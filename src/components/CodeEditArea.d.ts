/**
 * An editable, syntax-highlighted code surface.
 *
 * Built as a transparent `<textarea>` sitting exactly on top of a highlighted `<pre>`,
 * with the two kept in scroll lock. That keeps every native editing behaviour the
 * browser already gets right — the caret, IME composition, undo history, selection,
 * accessibility, spellcheck control — while the colour comes from the copy underneath.
 * The alternative, a contentEditable rich surface, has to reimplement all of that and
 * gets it subtly wrong.
 *
 * It reuses the `highlight.js` instance and the extension→language map the agent panel
 * already registers, so the two views highlight identically and no new dependency is
 * needed for it.
 */
export declare function CodeEditArea({ value, onChange, path, readOnly, onKeyDown, }: {
    value: string;
    onChange: (next: string) => void;
    /** Used to pick the language from the file extension. */
    path: string;
    readOnly?: boolean;
    /** Extra key handling (e.g. Ctrl+Enter to run). Return true to stop default Tab handling. */
    onKeyDown?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void | boolean;
}): import("react").JSX.Element;
//# sourceMappingURL=CodeEditArea.d.ts.map