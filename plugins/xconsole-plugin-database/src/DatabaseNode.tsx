import { useState } from "react";
import { DatabaseTree, type DbInstance } from "./DatabaseTree";
import { DatabaseIcon } from "./icons";

interface QueryResult {
  columns: string[];
  rows: Record<string, any>[];
  executionTimeMs: number;
  rowCount: number;
}

export function DatabaseNode({
  onClose,
}: {
  onClose?: () => void;
}) {
  const [instances, setInstances] = useState<DbInstance[]>([
    {
      endpoint: {
        id: "db-mysql-prod",
        vps_id: "vps-1",
        product: "MySQL Server",
        kind: "native",
        engine: "mysql",
        host: "127.0.0.1",
        port: 3306,
        container: "production-mysql",
      },
      sessionId: "session-1",
      version: "8.0.35",
      schemas: ["production_app", "sys", "information_schema"],
      tables: {
        production_app: [
          { name: "users", rows: 1420, bytes: 65536, engine: "InnoDB" },
          { name: "sessions", rows: 840, bytes: 32768, engine: "InnoDB" },
          { name: "orders", rows: 5230, bytes: 131072, engine: "InnoDB" },
          { name: "audit_logs", rows: 18450, bytes: 524288, engine: "InnoDB" },
        ],
      },
      expanded: true,
      openSchemas: ["production_app"],
      busy: false,
      error: null,
    },
  ]);

  const [selectedTable, setSelectedTable] = useState<string>("users");
  const [selectedSchema, setSelectedSchema] = useState<string>("production_app");
  const [sqlQuery, setSqlQuery] = useState<string>("SELECT * FROM users LIMIT 25;");
  const [activeTab, setActiveTab] = useState<"data" | "sql" | "schema">("data");
  const [executing, setExecuting] = useState(false);

  const [queryResult, setQueryResult] = useState<QueryResult>({
    columns: ["id", "email", "username", "role", "is_active", "created_at"],
    rows: [
      { id: 1, email: "admin@xconsole.dev", username: "admin", role: "superadmin", is_active: true, created_at: "2026-01-10 10:24:00" },
      { id: 2, email: "developer@xconsole.dev", username: "alex", role: "developer", is_active: true, created_at: "2026-02-14 14:15:30" },
      { id: 3, email: "operator@xconsole.dev", username: "sarah", role: "operator", is_active: false, created_at: "2026-03-01 09:00:12" },
      { id: 4, email: "support@xconsole.dev", username: "mark", role: "support", is_active: true, created_at: "2026-03-12 18:42:10" },
    ],
    executionTimeMs: 12,
    rowCount: 4,
  });

  const handleExecuteQuery = () => {
    setExecuting(true);
    setTimeout(() => {
      setExecuting(false);
      setQueryResult({
        columns: ["id", "email", "username", "role", "is_active", "created_at"],
        rows: [
          { id: 1, email: "admin@xconsole.dev", username: "admin", role: "superadmin", is_active: true, created_at: "2026-01-10 10:24:00" },
          { id: 2, email: "developer@xconsole.dev", username: "alex", role: "developer", is_active: true, created_at: "2026-02-14 14:15:30" },
          { id: 3, email: "operator@xconsole.dev", username: "sarah", role: "operator", is_active: false, created_at: "2026-03-01 09:00:12" },
          { id: 4, email: "support@xconsole.dev", username: "mark", role: "support", is_active: true, created_at: "2026-03-12 18:42:10" },
        ],
        executionTimeMs: Math.floor(Math.random() * 20) + 4,
        rowCount: 4,
      });
    }, 250);
  };

  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-2xl font-mono text-xs select-none">
      {/* Top Header */}
      <div className="flex h-9 items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-3 shrink-0">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <DatabaseIcon size={14} className="text-violet-400 shrink-0" />
          <span className="font-semibold text-gray-200 truncate font-sans text-xs">
            Database Inspector &amp; Query Console
          </span>
          <span className="rounded bg-zinc-800 text-zinc-400 border border-zinc-700 px-1.5 py-0.2 text-[10px]">
            {selectedSchema}.{selectedTable}
          </span>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <div className="flex rounded bg-black/40 border border-[var(--border)] p-0.5 text-[10px]">
            <button
              onClick={() => setActiveTab("data")}
              className={`rounded px-2 py-0.5 transition ${
                activeTab === "data" ? "bg-zinc-200 text-zinc-950 font-bold" : "text-zinc-400 hover:text-white"
              }`}
            >
              Table Data
            </button>
            <button
              onClick={() => setActiveTab("sql")}
              className={`rounded px-2 py-0.5 transition ${
                activeTab === "sql" ? "bg-zinc-200 text-zinc-950 font-bold" : "text-zinc-400 hover:text-white"
              }`}
            >
              SQL Console
            </button>
          </div>

          {onClose && (
            <button
              onClick={onClose}
              className="rounded p-1 text-zinc-400 hover:text-red-400 hover:bg-white/5 ml-1"
            >
              ✕
            </button>
          )}
        </div>
      </div>

      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left Database / Schema Tree */}
        <DatabaseTree
          instances={instances}
          scanning={false}
          onPatch={(epId, patch) => {
            setInstances((cur) =>
              cur.map((i) => (i.endpoint.id === epId ? { ...i, ...patch } : i))
            );
          }}
          onSelectTable={(_inst, schema, table) => {
            setSelectedSchema(schema);
            setSelectedTable(table);
            setSqlQuery(`SELECT * FROM ${table} LIMIT 25;`);
          }}
          onRescan={() => {}}
        />

        {/* Right Content Area */}
        <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
          {activeTab === "sql" ? (
            /* SQL Console Tab */
            <div className="flex-1 flex flex-col p-3 space-y-2 overflow-hidden">
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-zinc-300">Run SQL Query</span>
                <button
                  onClick={handleExecuteQuery}
                  disabled={executing}
                  className="rounded bg-zinc-100 hover:bg-white text-zinc-950 font-bold px-3 py-1 text-xs transition flex items-center gap-1"
                >
                  <span>{executing ? "Running…" : "▶ Execute"}</span>
                </button>
              </div>

              <textarea
                value={sqlQuery}
                onChange={(e) => setSqlQuery(e.target.value)}
                className="h-28 w-full resize-none rounded-lg border border-[var(--border)] bg-black/60 p-2.5 font-mono text-xs text-cyan-300 outline-none leading-relaxed focus:border-zinc-400"
              />

              <div className="flex-1 overflow-auto rounded-lg border border-[var(--border)] bg-[var(--surface-2)]">
                <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-[10px] text-zinc-400">
                  <span>{queryResult.rowCount} rows returned</span>
                  <span>Executed in {queryResult.executionTimeMs}ms</span>
                </div>
                <table className="w-full text-left text-xs">
                  <thead>
                    <tr className="border-b border-[var(--border)] text-zinc-500 text-[10px] uppercase">
                      {queryResult.columns.map((col) => (
                        <th key={col} className="p-2 font-medium">{col}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-[var(--border)]/40 text-[11px]">
                    {queryResult.rows.map((row, i) => (
                      <tr key={i} className="hover:bg-white/5 transition">
                        {queryResult.columns.map((col) => (
                          <td key={col} className="p-2 text-zinc-300">
                            {typeof row[col] === "boolean" ? (
                              <span className={row[col] ? "text-emerald-400" : "text-red-400"}>
                                {String(row[col])}
                              </span>
                            ) : (
                              String(row[col])
                            )}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          ) : (
            /* Table Data Grid Tab */
            <div className="flex-1 flex flex-col overflow-hidden">
              <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-xs">
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-gray-200">Table: {selectedTable}</span>
                  <span className="text-[10px] text-zinc-500">({queryResult.rowCount} records)</span>
                </div>

                <div className="flex items-center gap-2">
                  <button
                    onClick={handleExecuteQuery}
                    className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-300 border border-zinc-700 px-2 py-0.5 text-[10px]"
                  >
                    Refresh
                  </button>
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText(JSON.stringify(queryResult.rows, null, 2));
                    }}
                    className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-300 border border-zinc-700 px-2 py-0.5 text-[10px]"
                  >
                    Export JSON
                  </button>
                </div>
              </div>

              <div className="flex-1 overflow-auto p-2">
                <table className="w-full text-left text-xs border-collapse">
                  <thead>
                    <tr className="border-b border-[var(--border)] text-zinc-500 text-[10px] uppercase bg-[var(--surface-2)]">
                      {queryResult.columns.map((col) => (
                        <th key={col} className="p-2 font-medium">{col}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-[var(--border)]/40 text-[11px]">
                    {queryResult.rows.map((row, i) => (
                      <tr key={i} className="hover:bg-[var(--surface-hover)] cursor-pointer transition">
                        {queryResult.columns.map((col) => (
                          <td key={col} className="p-2 text-zinc-300">
                            {typeof row[col] === "boolean" ? (
                              <span className={row[col] ? "text-emerald-400 font-bold" : "text-red-400"}>
                                {String(row[col])}
                              </span>
                            ) : (
                              String(row[col])
                            )}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="flex items-center justify-between border-t border-[var(--border)] px-3 py-1.5 bg-[var(--surface-2)] text-[10px] text-zinc-500 shrink-0">
                <div>Page 1 of 1 (Showing 1-4)</div>
                <div>Connected to {selectedSchema}</div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
