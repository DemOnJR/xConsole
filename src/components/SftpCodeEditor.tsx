import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { api, type SftpEntry } from "../lib/tauri";
import { dialog } from "../stores/dialogStore";

/** Decode base64 (raw file bytes) into UTF-8 text. */
function bytesToText(b64: string): string {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i += 1) bytes[i] = bin.charCodeAt(i);
  return new TextDecoder().decode(bytes);
}

/** Encode UTF-8 text into base64 (chunked so large files don't blow the stack). */
function textToB64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

interface Props {
  sessionId: string;
  entry: SftpEntry;
  onClose: () => void;
  onSaved?: () => void;
}

/**
 * Minimal in-app remote code editor: opens a file over the live SFTP session,
 * edits it in place, and writes it back via `sftp_write`. Intentionally
 * dependency-free (a Monaco-backed surface is a drop-in upgrade later).
 */
export function SftpCodeEditor({ sessionId, entry, onClose, onSaved }: Props) {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [original, setOriginal] = useState("");
  const dirty = content !== original;

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const b64 = await api.sftpDownload(sessionId, entry.path);
        if (!mounted) return;
        const text = bytesToText(b64);
        setContent(text);
        setOriginal(text);
      } catch (e) {
        if (mounted) setError(String(e));
      } finally {
        if (mounted) setLoading(false);
      }
    })();
    return () => {
      mounted = false;
    };
  }, [sessionId, entry.path]);

  const save = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      await api.sftpWrite(sessionId, entry.path, textToB64(content));
      setOriginal(content);
      onSaved?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [sessionId, entry.path, content, onSaved]);

  const tryClose = useCallback(async () => {
    if (
      dirty &&
      !(await dialog.confirm({
        title: "Discard changes",
        message: "Discard unsaved changes?",
        danger: true,
        confirmText: "Discard",
      }))
    )
      return;
    onClose();
  }, [dirty, onClose]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (dirty && !saving && !loading) void save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dirty, saving, loading, save]);

  return createPortal(
    <div
      className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/50 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) tryClose();
      }}
    >
      <div
        className="flex h-[80vh] w-full max-w-4xl flex-col rounded-lg border border-[var(--border)] bg-[var(--bg)] shadow-xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-[var(--border)] px-4 py-2">
          <h3 className="text-sm font-medium text-gray-200">Edit</h3>
          <span className="truncate font-mono text-[10px] text-gray-500">{entry.path}</span>
          {dirty && <span className="text-[10px] text-amber-400">● unsaved</span>}
          <div className="ml-auto flex items-center gap-2">
            <button
              type="button"
              className="rounded bg-cyan-700 px-3 py-1 text-xs text-white hover:bg-cyan-600 disabled:opacity-40"
              onClick={() => void save()}
              disabled={!dirty || saving || loading}
            >
              {saving ? "Saving…" : "Save"}
            </button>
            <button
              type="button"
              className="rounded px-3 py-1 text-xs text-gray-400 hover:bg-[var(--border)]"
              onClick={tryClose}
            >
              Close
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 p-2">
          {loading ? (
            <p className="p-2 text-xs text-gray-500">Loading…</p>
          ) : (
            <textarea
              value={content}
              spellCheck={false}
              onChange={(e) => setContent(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Tab") {
                  e.preventDefault();
                  const el = e.currentTarget;
                  const { selectionStart: s, selectionEnd: en } = el;
                  const next = `${content.slice(0, s)}  ${content.slice(en)}`;
                  setContent(next);
                  requestAnimationFrame(() => {
                    el.selectionStart = el.selectionEnd = s + 2;
                  });
                }
              }}
              className="h-full w-full resize-none rounded bg-[var(--bg)] p-3 font-mono text-xs leading-relaxed text-gray-200 outline-none"
            />
          )}
        </div>

        <div className="flex items-center gap-2 border-t border-[var(--border)] px-4 py-1.5 text-[10px] text-gray-600">
          <span>Ctrl/⌘+S to save · written over SSH (SFTP)</span>
          {error && <span className="ml-auto text-red-300">{error}</span>}
        </div>
      </div>
    </div>,
    document.body,
  );
}
