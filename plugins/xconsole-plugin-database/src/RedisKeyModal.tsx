import { useEffect, useState } from "react";
import type { DbResultSet } from "../../../src/lib/tauri";

interface RedisKeyModalProps {
  open: boolean;
  sessionId: string;
  schema: string;
  keyName: string;
  initialType?: string;
  initialTtl?: string;
  onClose: () => void;
  onSubmitSql: (sql: string) => Promise<DbResultSet>;
  onSuccess: () => void;
}

export function RedisKeyModal({
  open,
  sessionId: _sessionId,
  schema: _schema,
  keyName,
  initialType = "string",
  initialTtl = "-1",
  onClose,
  onSubmitSql,
  onSuccess,
}: RedisKeyModalProps) {
  const [keyType, setKeyType] = useState(initialType);
  const [ttl, setTtl] = useState(initialTtl);
  const [rawContent, setRawContent] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [formatJson, setFormatJson] = useState(false);

  useEffect(() => {
    if (!open || !keyName) return;
    setBusy(true);
    setError(null);

    const loadKey = async () => {
      try {
        const escapedKey = keyName.replace(/'/g, "\\'");
        // Fetch TYPE and TTL
        const typeRes = await onSubmitSql(`TYPE '${escapedKey}'`);
        const typeStr = typeRes.rows[0]?.[0] ?? initialType;
        setKeyType(typeStr);

        const ttlRes = await onSubmitSql(`TTL '${escapedKey}'`);
        const ttlStr = ttlRes.rows[0]?.[0] ?? initialTtl;
        setTtl(ttlStr);

        // Fetch value based on type
        let valRes: DbResultSet;
        switch (typeStr) {
          case "hash":
            valRes = await onSubmitSql(`HGETALL '${escapedKey}'`);
            break;
          case "list":
            valRes = await onSubmitSql(`LRANGE '${escapedKey}' 0 200`);
            break;
          case "set":
            valRes = await onSubmitSql(`SMEMBERS '${escapedKey}'`);
            break;
          case "zset":
            valRes = await onSubmitSql(`ZRANGE '${escapedKey}' 0 200 WITHSCORES`);
            break;
          case "string":
          default:
            valRes = await onSubmitSql(`GET '${escapedKey}'`);
            break;
        }

        const lines = valRes.rows.map((r) => r[0] ?? "").join("\n");
        setRawContent(lines);

        // Check if JSON
        try {
          if (typeStr === "string" && lines.trim().startsWith("{")) {
            JSON.parse(lines);
            setFormatJson(true);
          }
        } catch {
          setFormatJson(false);
        }
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    };

    void loadKey();
  }, [open, keyName]);

  if (!open) return null;

  const handleSave = async () => {
    setBusy(true);
    setError(null);
    try {
      const escapedKey = keyName.replace(/'/g, "\\'");
      const escapedVal = rawContent.replace(/'/g, "\\'");

      if (keyType === "string") {
        await onSubmitSql(`SET '${escapedKey}' '${escapedVal}'`);
      } else {
        setError("Direct text editing is only supported for String keys currently.");
        setBusy(false);
        return;
      }

      // Update TTL if changed
      const ttlNum = Number(ttl);
      if (!isNaN(ttlNum)) {
        if (ttlNum > 0) {
          await onSubmitSql(`EXPIRE '${escapedKey}' ${ttlNum}`);
        } else if (ttlNum === -1) {
          await onSubmitSql(`PERSIST '${escapedKey}'`);
        }
      }

      onSuccess();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    if (!confirm(`Delete Redis key '${keyName}' permanently?`)) return;
    setBusy(true);
    setError(null);
    try {
      const escapedKey = keyName.replace(/'/g, "\\'");
      await onSubmitSql(`DEL '${escapedKey}'`);
      onSuccess();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  let formattedDisplay = rawContent;
  if (formatJson && keyType === "string") {
    try {
      formattedDisplay = JSON.stringify(JSON.parse(rawContent), null, 2);
    } catch {
      // ignore
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-xs"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex max-h-[90vh] w-full max-w-2xl flex-col rounded-lg border border-[var(--border)] bg-[var(--surface-elevated)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <div className="min-w-0 flex-1 pr-2">
            <div className="flex items-center gap-2">
              <h3 className="truncate font-mono text-sm font-semibold text-[var(--text)]">
                {keyName}
              </h3>
              <span className="shrink-0 rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] font-bold uppercase text-red-300">
                {keyType}
              </span>
            </div>
            <p className="text-[11px] text-[var(--text-dim)]">
              TTL: {ttl === "-1" ? "No Expiry (Permanent)" : `${ttl}s`}
            </p>
          </div>
          <button
            type="button"
            className="rounded p-1 text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
            onClick={onClose}
          >
            ✕
          </button>
        </div>

        {/* Error Banner */}
        {error ? (
          <div className="border-b border-red-500/30 bg-red-500/10 px-4 py-2 text-xs text-red-300">
            {error}
          </div>
        ) : null}

        {/* Form Body */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <label className="text-xs font-medium text-[var(--text-dim)]">TTL (seconds):</label>
              <input
                type="number"
                value={ttl}
                onChange={(e) => setTtl(e.target.value)}
                placeholder="-1"
                className="w-24 rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-0.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
              />
            </div>

            {keyType === "string" ? (
              <label className="flex items-center gap-1.5 text-xs text-[var(--text-dim)]">
                <input
                  type="checkbox"
                  checked={formatJson}
                  onChange={(e) => setFormatJson(e.target.checked)}
                  className="rounded border-[var(--border)] text-violet-600 focus:ring-0"
                />
                Format JSON
              </label>
            ) : null}
          </div>

          <div>
            <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
              Value Content
            </label>
            <textarea
              rows={12}
              value={formatJson ? formattedDisplay : rawContent}
              readOnly={keyType !== "string"}
              onChange={(e) => setRawContent(e.target.value)}
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] p-2.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
            />
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] px-4 py-3">
          <button
            type="button"
            disabled={busy}
            onClick={() => void handleDelete()}
            className="rounded bg-red-600/20 px-3 py-1.5 text-xs font-medium text-red-300 hover:bg-red-600/30 disabled:opacity-40"
          >
            Delete Key
          </button>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="rounded px-3 py-1.5 text-xs text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
              onClick={onClose}
            >
              Cancel
            </button>
            {keyType === "string" ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => void handleSave()}
                className="rounded bg-violet-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-violet-500 disabled:opacity-40"
              >
                {busy ? "Saving…" : "Save Changes"}
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

