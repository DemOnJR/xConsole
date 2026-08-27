import { useState } from "react";
import type { DbColumn, DbResultSet } from "../../../src/lib/tauri";

interface AddColumnModalProps {
  open: boolean;
  schema: string;
  table: string;
  existingColumns: DbColumn[];
  engine?: "mysql" | "postgres" | "redis";
  onClose: () => void;
  onSubmitSql: (sql: string) => Promise<DbResultSet>;
  onSuccess: () => void;
}

const COMMON_DATA_TYPES = [
  "VARCHAR",
  "INT",
  "BIGINT",
  "TEXT",
  "LONGTEXT",
  "DATETIME",
  "TIMESTAMP",
  "BOOLEAN",
  "DECIMAL",
  "JSON",
  "FLOAT",
  "DOUBLE",
  "BLOB",
  "UUID",
];

export function AddColumnModal({
  open,
  schema,
  table,
  existingColumns,
  engine = "mysql",
  onClose,
  onSubmitSql,
  onSuccess,
}: AddColumnModalProps) {
  const isPostgres = engine === "postgres";
  const [colName, setColName] = useState("");
  const [dataType, setDataType] = useState("VARCHAR");
  const [length, setLength] = useState("255");
  const [nullable, setNullable] = useState(true);
  const [defaultValue, setDefaultValue] = useState("");
  const [afterCol, setAfterCol] = useState<string>("");

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const buildSql = (): string => {
    const name = colName.trim();
    if (!name) return "";

    let typeDef = dataType;
    if (length && length.trim()) {
      typeDef += `(${length.trim()})`;
    }

    let line = `ALTER TABLE ${schema}.${table} ADD COLUMN ${name} ${typeDef}`;

    if (!nullable) {
      line += " NOT NULL";
    } else if (!isPostgres) {
      line += " NULL";
    }

    if (defaultValue && defaultValue.trim()) {
      const def = defaultValue.trim();
      if (
        def.toUpperCase() === "CURRENT_TIMESTAMP" ||
        def.toUpperCase() === "NOW()" ||
        def.toUpperCase() === "NULL" ||
        /^\d+(\.\d+)?$/.test(def)
      ) {
        line += ` DEFAULT ${def}`;
      } else {
        line += ` DEFAULT '${def.replace(/'/g, "''")}'`;
      }
    }

    if (!isPostgres && afterCol) {
      line += ` AFTER ${afterCol}`;
    }

    return `${line};`;
  };

  const handleAdd = async () => {
    setError(null);
    const name = colName.trim();
    if (!name) {
      setError("Please specify a column name.");
      return;
    }

    const sql = buildSql();
    setBusy(true);
    try {
      await onSubmitSql(sql);
      onSuccess();
      onClose();
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
      <div className="flex max-h-[90vh] w-full max-w-lg flex-col rounded-lg border border-[var(--border)] bg-[var(--surface-elevated)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <div>
            <h3 className="text-sm font-semibold text-[var(--text)]">
              Add Column to `{schema}.{table}`
            </h3>
            <p className="text-[11px] text-[var(--text-dim)]">
              Alter table schema to add a new column
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
          <div>
            <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
              Column Name
            </label>
            <input
              type="text"
              value={colName}
              onChange={(e) => setColName(e.target.value)}
              placeholder="e.g. status, bio, count..."
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
              autoFocus
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                Data Type
              </label>
              <select
                value={dataType}
                onChange={(e) => setDataType(e.target.value)}
                className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
              >
                {COMMON_DATA_TYPES.map((dt) => (
                  <option key={dt} value={dt}>
                    {dt}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                Length / Values
              </label>
              <input
                type="text"
                value={length}
                onChange={(e) => setLength(e.target.value)}
                placeholder="255"
                className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                Default Value
              </label>
              <input
                type="text"
                value={defaultValue}
                onChange={(e) => setDefaultValue(e.target.value)}
                placeholder="e.g. NULL, 'active', 0"
                className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
              />
            </div>
            {!isPostgres ? (
              <div>
                <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
                  Position (After Column)
                </label>
                <select
                  value={afterCol}
                  onChange={(e) => setAfterCol(e.target.value)}
                  className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                >
                  <option value="">(End of table)</option>
                  {existingColumns.map((c) => (
                    <option key={c.name} value={c.name}>
                      After {c.name}
                    </option>
                  ))}
                </select>
              </div>
            ) : null}
          </div>

          <div className="flex items-center gap-2 pt-1">
            <input
              type="checkbox"
              id="col-nullable"
              checked={nullable}
              onChange={(e) => setNullable(e.target.checked)}
              className="h-4 w-4 rounded border-[var(--border)] text-violet-600 focus:ring-0"
            />
            <label htmlFor="col-nullable" className="text-xs text-[var(--text)]">
              Allow NULL values
            </label>
          </div>

          {/* DDL Preview */}
          <div className="rounded border border-[var(--border)] bg-[var(--bg)]/50 p-2.5">
            <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
              Generated ALTER SQL
            </div>
            <pre className="overflow-x-auto font-mono text-[11px] text-violet-300 whitespace-pre-wrap">
              {buildSql() || "-- Fill out column details to preview SQL"}
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
          <button
            type="button"
            disabled={busy || !colName.trim()}
            onClick={() => void handleAdd()}
            className="rounded bg-violet-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-violet-500 disabled:opacity-40"
          >
            {busy ? "Adding Column…" : "Add Column"}
          </button>
        </div>
      </div>
    </div>
  );
}

