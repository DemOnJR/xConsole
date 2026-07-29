import { useEffect, useMemo, useRef, useState } from "react";
import hljs from "highlight.js/lib/core";
import { langFromPath } from "./agent/SyntaxHighlight";

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
export function CodeEditArea({
  value,
  onChange,
  path,
  readOnly,
}: {
  value: string;
  onChange: (next: string) => void;
  /** Used to pick the language from the file extension. */
  path: string;
  readOnly?: boolean;
}) {
  const taRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);
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
    } catch {
      // Never let a highlighter failure cost the user their editor.
      return escapeHtml(source);
    }
  }, [value, language]);

  const lineCount = useMemo(() => value.split("\n").length, [value]);

  // Keep the highlighted layer and the gutter aligned with the textarea.
  useEffect(() => {
    const ta = taRef.current;
    if (!ta) return;
    const sync = () => {
      if (preRef.current) {
        preRef.current.scrollTop = ta.scrollTop;
        preRef.current.scrollLeft = ta.scrollLeft;
      }
      if (gutterRef.current) gutterRef.current.scrollTop = ta.scrollTop;
    };
    ta.addEventListener("scroll", sync);
    sync();
    return () => ta.removeEventListener("scroll", sync);
  }, []);

  const shared = `m-0 font-mono text-xs leading-[1.55] ${
    wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre"
  }`;

  return (
    <div className="relative flex h-full min-h-0 overflow-hidden rounded border border-[var(--border)] bg-[var(--bg)]">
      {/* Line numbers. aria-hidden so a screen reader reads the code, not the digits. */}
      <div
        ref={gutterRef}
        aria-hidden
        className="shrink-0 overflow-hidden border-r border-[var(--border)] bg-[var(--surface)] py-3 pl-2 pr-2 text-right font-mono text-xs leading-[1.55] text-gray-600 select-none"
      >
        {Array.from({ length: lineCount }, (_, i) => (
          <div key={i}>{i + 1}</div>
        ))}
      </div>

      <div className="relative min-w-0 flex-1">
        <pre
          ref={preRef}
          aria-hidden
          className={`pointer-events-none absolute inset-0 overflow-auto p-3 ${shared}`}
        >
          <code dangerouslySetInnerHTML={{ __html: html }} />
        </pre>

        <textarea
          ref={taRef}
          value={value}
          readOnly={readOnly}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Tab") {
              e.preventDefault();
              const el = e.currentTarget;
              const { selectionStart: s, selectionEnd: en } = el;
              onChange(`${value.slice(0, s)}  ${value.slice(en)}`);
              requestAnimationFrame(() => {
                el.selectionStart = el.selectionEnd = s + 2;
              });
            }
          }}
          // Transparent text with a visible caret: the colour comes from the <pre>
          // underneath, but the caret and selection must stay legible.
          className={`absolute inset-0 h-full w-full resize-none overflow-auto bg-transparent p-3 text-transparent caret-gray-100 outline-none selection:bg-cyan-500/30 ${shared}`}
        />

        <div className="pointer-events-none absolute bottom-1 right-2 flex items-center gap-2">
          <button
            type="button"
            onClick={() => setWrap((w) => !w)}
            className="pointer-events-auto rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-gray-400 hover:text-white"
            data-tooltip={wrap ? "Don't wrap long lines" : "Wrap long lines"}
          >
            {wrap ? "no wrap" : "wrap"}
          </button>
          <span className="rounded bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-gray-500">
            {language ?? "plain text"}
          </span>
        </div>
      </div>
    </div>
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
