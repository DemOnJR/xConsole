import { useState } from "react";
import type { DbColumn, DbResultSet } from "../../lib/tauri";

interface InsertRowModalProps {
  open: boolean;
  schema: string;
  table: string;
  columns: DbColumn[];
  isRedis?: boolean;
  onClose: () => void;
  onSubmitSql: (sql: string) => Promise<DbResultSet>;
  onSuccess: () => void;
}

export function InsertRowModal({
  open,
  schema,
  table,
  columns,
  isRedis = false,
  onClose,
  onSubmitSql,
  onSuccess,
}: InsertRowModalProps) {
  // State for SQL column inputs
  const [values, setValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {};
    for (const col of columns) {
      if (col.default && col.default !== "NULL" && !col.default.startsWith("nextval(")) {
        init[col.name] = col.default.replace(/^'(.*)'$/, "$1");
      } else {
        init[col.name] = "";
      }
    }
    return init;
  });

  const [nullFlags, setNullFlags] = useState<Record<string, boolean>>(() => {
    const init: Record<string, boolean> = {};
    for (const col of columns) {
      // Auto-increment / sequence columns default to NULL or DEFAULT
      if (col.extra.toLowerCase().includes("auto_increment") || col.default.startsWith("nextval(")) {
        init[col.name] = true;
      } else {
        init[col.name] = col.nullable && !col.default;
      }
    }
    return init;
  });

  // State for Redis key insert
  const [redisKey, setRedisKey] = useState("");
  const [redisType, setRedisType] = useState<"string" | "hash" | "list" | "set" | "zset">("string");
  const [redisTtl, setRedisTtl] = useState("");
  const [redisValue, setRedisValue] = useState("");
  const [redisField, setRedisField] = useState(""); // for hash / zset score

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const buildSql = (): string => {
    if (isRedis) {
      const k = redisKey.trim();
      const v = redisValue.trim();
      if (!k) return "";
      let cmd = "";
      switch (redisType) {
        case "string":
          cmd = `SET ${k} '${v.replace(/'/g, "\\'")}'`;
          break;
        case "hash":
          cmd = `HSET ${k} ${redisField.trim() || "field"} '${v.replace(/'/g, "\\'")}'`;
          break;
        case "list":
          cmd = `RPUSH ${k} '${v.replace(/'/g, "\\'")}'`;
          break;
        case "set":
          cmd = `SADD ${k} '${v.replace(/'/g, "\\'")}'`;
          break;
        case "zset":
          cmd = `ZADD ${k} ${redisField.trim() || "1"} '${v.replace(/'/g, "\\'")}'`;
          break;
      }
      if (redisTtl && Number(redisTtl) > 0) {
        cmd += `\nEXPIRE ${k} ${Number(redisTtl)}`;
      }
      return cmd;
    }

    const activeCols: string[] = [];
    const activeVals: string[] = [];

    for (const col of columns) {
      const isNull = nullFlags[col.name];
      const val = values[col.name] ?? "";

      // Skip auto-increment columns if left empty/null
      if (
        (col.extra.toLowerCase().includes("auto_increment") || col.default.startsWith("nextval(")) &&
        isNull
      ) {
        continue;
      }

      activeCols.push(col.name);
      if (isNull) {
        activeVals.push("NULL");
      } else {
        activeVals.push(`'${val.replace(/'/g, "''")}'`);
      }
    }

    if (activeCols.length === 0) {
      return `INSERT INTO ${schema}.${table} DEFAULT VALUES;`;
    }

    return `INSERT INTO ${schema}.${table} (${activeCols.join(", ")}) VALUES (${activeVals.join(", ")});`;
  };

  const handleSave = async (stayOpen: boolean = false) => {
    setError(null);
    const sql = buildSql();
    if (!sql.trim()) {
      setError("Please fill out the required fields.");
      return;
    }

    setBusy(true);
    try {
      if (isRedis) {
        // Execute line by line for multi-command redis
        const cmds = sql.split("\n").filter((c) => c.trim().length > 0);
        for (const cmd of cmds) {
          await onSubmitSql(cmd);
        }
      } else {
        await onSubmitSql(sql);
      }
      onSuccess();
      if (stayOpen) {
        // Reset values for another insert
        setValues({});
        setNullFlags({});
      } else {
        onClose();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

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
          <div>
            <h3 className="text-sm font-semibold text-[var(--text)]">
              {isRedis ? "Add Redis Key" : `Insert Row · ${schema}.${table}`}
            </h3>
            <p className="text-[11px] text-[var(--text-dim)]">
              {isRedis
                ? `Create a new key under database '${schema}'`
                : `${columns.length} column${columns.length === 1 ? "" : "s"} in table`}
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
        <div className="flex-1 overflow-y-auto p-4">
          {isRedis ? (
            <div className="space-y-3">
              <div>
                <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                  Key Name
                </label>
                <input
                  type="text"
                  value={redisKey}
                  onChange={(e) => setRedisKey(e.target.value)}
                  placeholder="e.g. session:user:123"
                  className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                  autoFocus
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                    Type
                  </label>
                  <select
                    value={redisType}
                    onChange={(e) => setRedisType(e.target.value as any)}
                    className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                  >
                    <option value="string">String</option>
                    <option value="hash">Hash</option>
                    <option value="list">List</option>
                    <option value="set">Set</option>
                    <option value="zset">Sorted Set (ZSet)</option>
                  </select>
                </div>
                <div>
                  <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                    TTL (seconds, optional)
                  </label>
                  <input
                    type="number"
                    value={redisTtl}
                    onChange={(e) => setRedisTtl(e.target.value)}
                    placeholder="e.g. 3600 (-1 = permanent)"
                    className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                  />
                </div>
              </div>

              {redisType === "hash" ? (
                <div>
                  <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                    Field Name
                  </label>
                  <input
                    type="text"
                    value={redisField}
                    onChange={(e) => setRedisField(e.target.value)}
                    placeholder="Field name (e.g. email)"
                    className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                  />
                </div>
              ) : null}

              {redisType === "zset" ? (
                <div>
                  <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                    Score
                  </label>
                  <input
                    type="number"
                    value={redisField}
                    onChange={(e) => setRedisField(e.target.value)}
                    placeholder="Score (e.g. 100)"
                    className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                  />
                </div>
              ) : null}

              <div>
                <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                  Value
                </label>
                <textarea
                  rows={4}
                  value={redisValue}
                  onChange={(e) => setRedisValue(e.target.value)}
                  placeholder="Enter key value..."
                  className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                />
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              <div className="grid grid-cols-12 gap-2 border-b border-[var(--border)] pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
                <span className="col-span-4">Column</span>
                <span className="col-span-2">Type</span>
                <span className="col-span-5">Value</span>
                <span className="col-span-1 text-center">Null</span>
              </div>

              {columns.map((col) => {
                const isAuto =
                  col.extra.toLowerCase().includes("auto_increment") ||
                  col.default.startsWith("nextval(");
                const isNull = Boolean(nullFlags[col.name]);
                const val = values[col.name] ?? "";

                return (
                  <div
                    key={col.name}
                    className="grid grid-cols-12 items-center gap-2 rounded py-1 hover:bg-[var(--surface-hover)]/30"
                  >
                    <div className="col-span-4 flex items-center gap-1.5 truncate">
                      <span className="truncate text-xs font-medium text-[var(--text)]">
                        {col.name}
                      </span>
                      {col.primary ? (
                        <span className="shrink-0 rounded bg-amber-500/20 px-1 text-[9px] font-bold text-amber-400">
                          PK
                        </span>
                      ) : null}
                      {isAuto ? (
                        <span className="shrink-0 rounded bg-violet-500/20 px-1 text-[9px] text-violet-300">
                          auto
                        </span>
                      ) : null}
                    </div>

                    <div className="col-span-2 truncate font-mono text-[10px] text-[var(--text-dim)]">
                      {col.data_type}
                    </div>

                    <div className="col-span-5">
                      <input
                        type="text"
                        disabled={isNull}
                        value={val}
                        placeholder={
                          isAuto
                            ? "(auto-generated)"
                            : col.default
                            ? `Default: ${col.default}`
                            : isNull
                            ? "NULL"
                            : ""
                        }
                        onChange={(e) =>
                          setValues((prev) => ({ ...prev, [col.name]: e.target.value }))
                        }
                        className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 font-mono text-xs text-[var(--text)] placeholder:text-gray-600 disabled:opacity-40 focus:border-violet-500 focus:outline-hidden"
                      />
                    </div>

                    <div className="col-span-1 flex justify-center">
                      {col.nullable || isAuto ? (
                        <input
                          type="checkbox"
                          checked={isNull}
                          onChange={(e) =>
                            setNullFlags((prev) => ({ ...prev, [col.name]: e.target.checked }))
                          }
                          className="h-3.5 w-3.5 rounded border-[var(--border)] text-violet-600 focus:ring-0"
                          title={isNull ? "Value is NULL" : "Click to set NULL"}
                        />
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* SQL Preview */}
          <div className="mt-4 rounded border border-[var(--border)] bg-[var(--bg)]/50 p-2.5">
            <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
              Live Preview
            </div>
            <pre className="overflow-x-auto font-mono text-[11px] text-violet-300 whitespace-pre-wrap">
              {buildSql() || "(empty statement)"}
            </pre>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] px-4 py-3">
          <button
            type="button"
            className="rounded px-3 py-1.5 text-xs text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
            onClick={onClose}
          >
            Cancel
          </button>
          <div className="flex items-center gap-2">
            {!isRedis ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => void handleSave(true)}
                className="rounded border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-xs font-medium text-[var(--text)] hover:bg-[var(--surface-hover)] disabled:opacity-40"
              >
                Insert & Next
              </button>
            ) : null}
            <button
              type="button"
              disabled={busy}
              onClick={() => void handleSave(false)}
              className="rounded bg-violet-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-violet-500 disabled:opacity-40"
            >
              {busy ? "Inserting…" : "Insert Row"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
