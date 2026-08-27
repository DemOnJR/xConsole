import { useState } from "react";
import type { DbResultSet } from "../../lib/tauri";

export interface ColumnDraft {
  id: string;
  name: string;
  dataType: string;
  length?: string;
  nullable: boolean;
  primaryKey: boolean;
  autoIncrement: boolean;
  defaultValue?: string;
}

interface CreateTableModalProps {
  open: boolean;
  schema: string;
  engine?: "mysql" | "postgres" | "redis";
  onClose: () => void;
  onSubmitSql: (sql: string) => Promise<DbResultSet>;
  onSuccess: (newTableName: string) => void;
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

export function CreateTableModal({
  open,
  schema,
  engine = "mysql",
  onClose,
  onSubmitSql,
  onSuccess,
}: CreateTableModalProps) {
  const isPostgres = engine === "postgres";
  const [tableName, setTableName] = useState("");
  const [columns, setColumns] = useState<ColumnDraft[]>([
    {
      id: "1",
      name: "id",
      dataType: isPostgres ? "BIGINT" : "BIGINT",
      length: "",
      nullable: false,
      primaryKey: true,
      autoIncrement: true,
      defaultValue: "",
    },
    {
      id: "2",
      name: "name",
      dataType: "VARCHAR",
      length: "255",
      nullable: false,
      primaryKey: false,
      autoIncrement: false,
      defaultValue: "",
    },
    {
      id: "3",
      name: "created_at",
      dataType: isPostgres ? "TIMESTAMP" : "DATETIME",
      length: "",
      nullable: false,
      primaryKey: false,
      autoIncrement: false,
      defaultValue: isPostgres ? "CURRENT_TIMESTAMP" : "CURRENT_TIMESTAMP",
    },
  ]);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const addColumn = () => {
    setColumns((prev) => [
      ...prev,
      {
        id: String(Date.now() + Math.random()),
        name: `col_${prev.length + 1}`,
        dataType: "VARCHAR",
        length: "255",
        nullable: true,
        primaryKey: false,
        autoIncrement: false,
        defaultValue: "",
      },
    ]);
  };

  const removeColumn = (id: string) => {
    if (columns.length <= 1) return;
    setColumns((prev) => prev.filter((c) => c.id !== id));
  };

  const updateColumn = (id: string, patch: Partial<ColumnDraft>) => {
    setColumns((prev) => prev.map((c) => (c.id === id ? { ...c, ...patch } : c)));
  };

  const buildSql = (): string => {
    const t = tableName.trim();
    if (!t) return "";

    const lines: string[] = [];
    const pks: string[] = [];

    for (const col of columns) {
      const name = col.name.trim();
      if (!name) continue;

      let typeDef = col.dataType;
      if (col.length && col.length.trim()) {
        typeDef += `(${col.length.trim()})`;
      }

      if (isPostgres) {
        if (col.autoIncrement && col.primaryKey) {
          typeDef = col.dataType === "INT" ? "SERIAL" : "BIGSERIAL";
        }
      }

      let line = `  ${name} ${typeDef}`;

      if (!col.nullable) {
        line += " NOT NULL";
      } else if (!isPostgres) {
        line += " NULL";
      }

      if (col.defaultValue && col.defaultValue.trim()) {
        const def = col.defaultValue.trim();
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

      if (!isPostgres && col.autoIncrement) {
        line += " AUTO_INCREMENT";
      }

      lines.push(line);

      if (col.primaryKey) {
        pks.push(name);
      }
    }

    if (pks.length > 0) {
      lines.push(`  PRIMARY KEY (${pks.join(", ")})`);
    }

    return `CREATE TABLE ${schema}.${t} (\n${lines.join(",\n")}\n);`;
  };

  const handleCreate = async () => {
    setError(null);
    const t = tableName.trim();
    if (!t) {
      setError("Please specify a table name.");
      return;
    }
    if (columns.some((c) => !c.name.trim())) {
      setError("All columns must have a name.");
      return;
    }

    const sql = buildSql();
    setBusy(true);
    try {
      await onSubmitSql(sql);
      onSuccess(t);
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
      <div className="flex max-h-[90vh] w-full max-w-3xl flex-col rounded-lg border border-[var(--border)] bg-[var(--surface-elevated)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <div>
            <h3 className="text-sm font-semibold text-[var(--text)]">
              Create Table in `{schema}`
            </h3>
            <p className="text-[11px] text-[var(--text-dim)]">
              Define table columns, keys, and default values ({isPostgres ? "PostgreSQL" : "MySQL"})
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
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          <div>
            <label className="mb-1 block text-xs font-medium text-[var(--text-dim)]">
              Table Name
            </label>
            <input
              type="text"
              value={tableName}
              onChange={(e) => setTableName(e.target.value)}
              placeholder="e.g. users, orders, logs..."
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
              autoFocus
            />
          </div>

          <div>
            <div className="mb-2 flex items-center justify-between">
              <label className="text-xs font-medium text-[var(--text-dim)]">Columns</label>
              <button
                type="button"
                onClick={addColumn}
                className="rounded bg-violet-600/20 px-2 py-0.5 text-[11px] font-medium text-violet-300 hover:bg-violet-600/30"
              >
                + Add Column
              </button>
            </div>

            <div className="space-y-1.5">
              <div className="grid grid-cols-12 gap-2 border-b border-[var(--border)] pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
                <span className="col-span-3">Name</span>
                <span className="col-span-3">Type</span>
                <span className="col-span-1">Length</span>
                <span className="col-span-2">Default</span>
                <span className="col-span-1 text-center">Null</span>
                <span className="col-span-1 text-center">PK</span>
                <span className="col-span-1 text-center">Action</span>
              </div>

              {columns.map((col) => (
                <div
                  key={col.id}
                  className="grid grid-cols-12 items-center gap-2 rounded py-1 hover:bg-[var(--surface-hover)]/20"
                >
                  <div className="col-span-3">
                    <input
                      type="text"
                      value={col.name}
                      placeholder="column_name"
                      onChange={(e) => updateColumn(col.id, { name: e.target.value })}
                      className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 font-mono text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                    />
                  </div>

                  <div className="col-span-3">
                    <select
                      value={col.dataType}
                      onChange={(e) => updateColumn(col.id, { dataType: e.target.value })}
                      className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-1 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                    >
                      {COMMON_DATA_TYPES.map((dt) => (
                        <option key={dt} value={dt}>
                          {dt}
                        </option>
                      ))}
                    </select>
                  </div>

                  <div className="col-span-1">
                    <input
                      type="text"
                      value={col.length ?? ""}
                      placeholder="255"
                      onChange={(e) => updateColumn(col.id, { length: e.target.value })}
                      className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-1 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                    />
                  </div>

                  <div className="col-span-2">
                    <input
                      type="text"
                      value={col.defaultValue ?? ""}
                      placeholder="NULL"
                      onChange={(e) => updateColumn(col.id, { defaultValue: e.target.value })}
                      className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-xs text-[var(--text)] focus:border-violet-500 focus:outline-hidden"
                    />
                  </div>

                  <div className="col-span-1 flex justify-center">
                    <input
                      type="checkbox"
                      checked={col.nullable}
                      disabled={col.primaryKey}
                      onChange={(e) => updateColumn(col.id, { nullable: e.target.checked })}
                      className="h-3.5 w-3.5 rounded border-[var(--border)] text-violet-600 focus:ring-0"
                    />
                  </div>

                  <div className="col-span-1 flex justify-center">
                    <input
                      type="checkbox"
                      checked={col.primaryKey}
                      onChange={(e) => {
                        const pk = e.target.checked;
                        updateColumn(col.id, {
                          primaryKey: pk,
                          nullable: pk ? false : col.nullable,
                          autoIncrement: pk ? col.autoIncrement : false,
                        });
                      }}
                      className="h-3.5 w-3.5 rounded border-[var(--border)] text-amber-500 focus:ring-0"
                    />
                  </div>

                  <div className="col-span-1 flex justify-center">
                    <button
                      type="button"
                      disabled={columns.length <= 1}
                      onClick={() => removeColumn(col.id)}
                      className="rounded px-1.5 py-0.5 text-xs text-red-400 hover:bg-red-950/40 hover:text-red-300 disabled:opacity-20"
                    >
                      ✕
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* DDL Preview */}
          <div className="rounded border border-[var(--border)] bg-[var(--bg)]/50 p-2.5">
            <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
              Generated SQL DDL
            </div>
            <pre className="overflow-x-auto font-mono text-[11px] text-violet-300 whitespace-pre-wrap">
              {buildSql() || "-- Fill out table name and columns to preview DDL"}
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
            disabled={busy || !tableName.trim()}
            onClick={() => void handleCreate()}
            className="rounded bg-violet-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-violet-500 disabled:opacity-40"
          >
            {busy ? "Creating Table…" : "Create Table"}
          </button>
        </div>
      </div>
    </div>
  );
}
