import { memo, useEffect, useState } from "react";
import type { ChatSnippet } from "../../lib/snippetDetect";
import { CodeHighlight } from "./SyntaxHighlight";

interface SnippetPreviewModalProps {
  snippet: ChatSnippet;
  onClose: () => void;
  onInsertInline?: (content: string) => void;
  onDelete?: () => void;
}

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const SnippetPreviewModal = memo(function SnippetPreviewModal({
  snippet,
  onClose,
  onInsertInline,
  onDelete,
}: SnippetPreviewModalProps) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [onClose]);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(snippet.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  const lines = snippet.content.split("\n");

  return (
    <div
      className="nowheel nopan nodrag fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4 animate-in fade-in duration-150"
      onClick={onClose}
      onWheel={(e) => e.stopPropagation()}
    >
      <div
        className="nowheel nopan nodrag flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-[var(--border)] bg-[#0b0f19] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
        onWheel={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface)]/80 px-4 py-3">
          <div className="flex items-center gap-2.5">
            <span className="rounded bg-cyan-500/20 px-2 py-0.5 font-mono text-[11px] font-semibold uppercase text-cyan-300">
              {snippet.language}
            </span>
            <span className="font-mono text-sm font-medium text-gray-200">
              {snippet.name}
            </span>
            <span className="text-xs text-gray-400">
              ({snippet.lineCount} lines · {humanSize(snippet.size)})
            </span>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleCopy}
              className="flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2.5 py-1 text-xs text-gray-300 hover:bg-[var(--border)] hover:text-white transition"
            >
              {copied ? (
                <>
                  <span className="text-emerald-400">✓</span> Copied
                </>
              ) : (
                <>Copy</>
              )}
            </button>
            {onDelete && (
              <button
                type="button"
                onClick={() => {
                  onDelete();
                  onClose();
                }}
                className="rounded border border-red-500/30 bg-red-500/10 px-2.5 py-1 text-xs text-red-300 hover:bg-red-500/20 transition"
              >
                Delete
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="flex h-7 w-7 items-center justify-center rounded text-gray-400 hover:bg-[var(--border)] hover:text-white transition"
              aria-label="Close modal"
            >
              ✕
            </button>
          </div>
        </div>

        {/* Code Content with Line Numbers */}
        <div
          className="nowheel nopan nodrag flex-1 overflow-auto bg-[#070a10] p-3 font-mono text-xs leading-5"
          onWheel={(e) => e.stopPropagation()}
        >
          <div className="flex">
            {/* Line numbers */}
            <div className="select-none pr-3 text-right text-gray-600 font-mono">
              {lines.map((_, i) => (
                <div key={i}>{i + 1}</div>
              ))}
            </div>

            {/* Code */}
            <div className="flex-1 overflow-x-auto pl-2 border-l border-[var(--border)]/40">
              <CodeHighlight
                code={snippet.content}
                language={snippet.language}
                className="block"
              />
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] bg-[var(--surface)]/50 px-4 py-2.5 text-xs">
          <div className="text-gray-400">
            Will be included as a code attachment with your message
          </div>

          <div className="flex items-center gap-2">
            {onInsertInline && (
              <button
                type="button"
                onClick={() => {
                  onInsertInline(snippet.content);
                  onDelete?.();
                  onClose();
                }}
                className="rounded border border-cyan-500/40 bg-cyan-500/10 px-3 py-1 text-cyan-300 hover:bg-cyan-500/20 transition"
                data-tooltip="Put this code directly into the chat text input"
              >
                Insert as plain text
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="rounded bg-blue-600 px-3 py-1 text-white hover:bg-blue-500 transition font-medium"
            >
              Done
            </button>
          </div>
        </div>
      </div>
    </div>
  );
});
