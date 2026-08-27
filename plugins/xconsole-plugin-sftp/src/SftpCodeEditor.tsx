import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, type SftpEntry } from "../../../src/lib/tauri";
import { dialog } from "../../../src/stores/dialogStore";
import { CodeEditArea } from "../../../src/components/CodeEditArea";
import {
  base64ToBytes,
  bytesToBase64,
  checkEncodingLoss,
  decodeBytes,
  detectEncoding,
  encodeText,
  SUPPORTED_ENCODINGS,
} from "../../../src/lib/encoding";

interface Props {
  sessionId: string;
  entry: SftpEntry;
  onClose: () => void;
  onSaved?: () => void;
}

/**
 * Remote code editor over live SFTP session:
 * - Automatic character encoding detection (UTF-8, UTF-16, Windows-1250/1251/1252/1256, EUC-KR, GBK, Shift-JIS)
 * - Interactive encoding badge & selector
 * - Preserves original encoding on save
 * - Data-loss safety checks and warnings when converting or saving across encodings
 */
export function SftpCodeEditor({ sessionId, entry, onClose, onSaved }: Props) {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [original, setOriginal] = useState("");
  const [encoding, setEncoding] = useState("utf-8");
  const [detectedEncoding, setDetectedEncoding] = useState("utf-8");
  const rawBytesRef = useRef<Uint8Array>(new Uint8Array(0));
  const dirty = content !== original;

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const b64 = await api.sftpDownload(sessionId, entry.path);
        if (!mounted) return;
        const bytes = base64ToBytes(b64);
        rawBytesRef.current = bytes;
        const detected = detectEncoding(bytes);
        setDetectedEncoding(detected);
        setEncoding(detected);
        const text = decodeBytes(bytes, detected);
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

  const handleEncodingChange = useCallback(
    async (newEncoding: string) => {
      if (newEncoding === encoding) return;

      // If user has unsaved edits, warn before re-interpreting from original bytes
      if (dirty) {
        const ok = await dialog.confirm({
          title: "Change File Encoding",
          message:
            "Re-decoding the file with a different character encoding will reset unsaved edits to the file's raw content. Do you want to continue?",
          confirmText: "Re-decode",
          danger: true,
        });
        if (!ok) return;
      }

      setEncoding(newEncoding);
      if (rawBytesRef.current.length > 0) {
        const text = decodeBytes(rawBytesRef.current, newEncoding);
        setContent(text);
        setOriginal(text);
      }
    },
    [encoding, dirty],
  );

  const save = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      // Check if target encoding cannot represent some characters in the file
      const loss = checkEncodingLoss(content, encoding);
      if (loss.hasLoss) {
        const sampleChars = loss.lostChars.map((c) => `"${c}"`).join(", ");
        const confirmed = await dialog.confirm({
          title: "Character Encoding Warning",
          message: `The selected encoding "${encoding}" cannot represent some characters in this file (e.g. ${sampleChars}). Saving in this encoding may corrupt these characters into question marks (?). Do you want to proceed?`,
          confirmText: "Save Anyway",
          danger: true,
        });
        if (!confirmed) {
          setSaving(false);
          return;
        }
      }

      const encodedBytes = encodeText(content, encoding);
      const b64 = bytesToBase64(encodedBytes);
      await api.sftpWrite(sessionId, entry.path, b64);
      rawBytesRef.current = encodedBytes;
      setOriginal(content);
      onSaved?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [sessionId, entry.path, content, encoding, onSaved]);

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
          {dirty && <span className="text-[10px] text-amber-400">GùÅ unsaved</span>}

          <div className="ml-auto flex items-center gap-2">
            {/* Encoding Selector */}
            <div className="flex items-center gap-1">
              <span className="text-[10px] text-gray-500">Charset:</span>
              <select
                aria-label="Character Encoding"
                className="rounded border border-[var(--border)] bg-[var(--card)] px-2 py-0.5 font-mono text-[11px] text-cyan-300 hover:border-cyan-500 focus:outline-none"
                value={encoding}
                onChange={(e) => void handleEncodingChange(e.target.value)}
                disabled={loading || saving}
              >
                {SUPPORTED_ENCODINGS.map((enc) => (
                  <option key={enc.id} value={enc.id}>
                    {enc.name} {enc.id === detectedEncoding ? "(detected)" : ""}
                  </option>
                ))}
              </select>
            </div>

            <button
              type="button"
              className="rounded bg-cyan-700 px-3 py-1 text-xs text-white hover:bg-cyan-600 disabled:opacity-40"
              onClick={() => void save()}
              disabled={!dirty || saving || loading}
            >
              {saving ? "SavingGÇª" : "Save"}
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
            <p className="p-2 text-xs text-gray-500">LoadingGÇª</p>
          ) : (
            <CodeEditArea value={content} onChange={setContent} path={entry.path} />
          )}
        </div>

        <div className="flex items-center gap-2 border-t border-[var(--border)] px-4 py-1.5 text-[10px] text-gray-600">
          <span>Ctrl/Gîÿ+S to save -+ encoding: <strong className="text-gray-400">{encoding}</strong></span>
          {error && <span className="ml-auto text-red-300">{error}</span>}
        </div>
      </div>
    </div>,
    document.body,
  );
}
