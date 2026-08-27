import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { useEffect, useMemo, useRef, useState } from "react";
import hljs from "highlight.js/lib/core";
import { langFromPath } from "../../plugins/xconsole-plugin-agent/src/SyntaxHighlight";
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
export function CodeEditArea({ value, onChange, path, readOnly, onKeyDown, }) {
    const taRef = useRef(null);
    const preRef = useRef(null);
    const gutterRef = useRef(null);
    const [wrap, setWrap] = useState(false);
    const language = useMemo(() => {
        const lang = langFromPath(path);
        // `getLanguage` guards against an extension mapping to a language this build
        // didn't register — highlighting an unregistered name throws.
        return lang && hljs.getLanguage(lang) ? lang : undefined;
    }, [path]);
    const html = useMemo(() => {
        // A trailing newline must render, or the highlighted copy ends one line short of
        // the textarea and the two drift apart at the bottom.
        const source = value.endsWith("\n") ? `${value}\n` : value;
        try {
            return language
                ? hljs.highlight(source, { language, ignoreIllegals: true }).value
                : escapeHtml(source);
        }
        catch {
            // Never let a highlighter failure cost the user their editor.
            return escapeHtml(source);
        }
    }, [value, language]);
    const lineCount = useMemo(() => value.split("\n").length, [value]);
    // Keep the highlighted layer and the gutter aligned with the textarea.
    useEffect(() => {
        const ta = taRef.current;
        if (!ta)
            return;
        const sync = () => {
            if (preRef.current) {
                preRef.current.scrollTop = ta.scrollTop;
                preRef.current.scrollLeft = ta.scrollLeft;
            }
            if (gutterRef.current)
                gutterRef.current.scrollTop = ta.scrollTop;
        };
        ta.addEventListener("scroll", sync);
        sync();
        return () => ta.removeEventListener("scroll", sync);
    }, []);
    const shared = `m-0 font-mono text-xs leading-[1.55] ${wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre"}`;
    return (_jsxs("div", { className: "relative flex h-full min-h-0 overflow-hidden rounded border border-[var(--border)] bg-[var(--bg)]", children: [_jsx("div", { ref: gutterRef, "aria-hidden": true, className: "shrink-0 overflow-hidden border-r border-[var(--border)] bg-[var(--surface)] py-3 pl-2 pr-2 text-right font-mono text-xs leading-[1.55] text-gray-600 select-none", children: Array.from({ length: lineCount }, (_, i) => (_jsx("div", { children: i + 1 }, i))) }), _jsxs("div", { className: "relative min-w-0 flex-1", children: [_jsx("pre", { ref: preRef, "aria-hidden": true, className: `pointer-events-none absolute inset-0 overflow-auto p-3 ${shared}`, children: _jsx("code", { dangerouslySetInnerHTML: { __html: html } }) }), _jsx("textarea", { ref: taRef, value: value, readOnly: readOnly, spellCheck: false, autoCapitalize: "off", autoCorrect: "off", onChange: (e) => onChange(e.target.value), onKeyDown: (e) => {
                            if (onKeyDown?.(e))
                                return;
                            if (e.key === "Tab") {
                                e.preventDefault();
                                const el = e.currentTarget;
                                const { selectionStart: s, selectionEnd: en } = el;
                                onChange(`${value.slice(0, s)}  ${value.slice(en)}`);
                                requestAnimationFrame(() => {
                                    el.selectionStart = el.selectionEnd = s + 2;
                                });
                            }
                        }, 
                        // Transparent text with a visible caret: the colour comes from the <pre>
                        // underneath, but the caret and selection must stay legible.
                        className: `absolute inset-0 h-full w-full resize-none overflow-auto bg-transparent p-3 text-transparent caret-gray-100 outline-none selection:bg-cyan-500/30 ${shared}` }), _jsxs("div", { className: "pointer-events-none absolute bottom-1 right-2 flex items-center gap-2", children: [_jsx("button", { type: "button", onClick: () => setWrap((w) => !w), className: "pointer-events-auto rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-gray-400 hover:text-white", "data-tooltip": wrap ? "Don't wrap long lines" : "Wrap long lines", children: wrap ? "no wrap" : "wrap" }), _jsx("span", { className: "rounded bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-gray-500", children: language ?? "plain text" })] })] })] }));
}
function escapeHtml(s) {
    return s
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");
}
//# sourceMappingURL=CodeEditArea.js.map