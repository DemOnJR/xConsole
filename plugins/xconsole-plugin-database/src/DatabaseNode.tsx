import { useState, useEffect } from "react";
import { DatabaseIcon, RefreshIcon, StarFilledIcon, StarOutlineIcon } from "./icons";

interface DbEndpoint {
  id: string;
  host: string;
  port: number;
  product: string;
  engine: "mysql" | "postgres" | "redis" | "sqlite";
}

interface QueryResult {
  columns: string[];
  rows: Record<string, any>[];
  executionTimeMs: number;
  rowCount: number;
  affected?: number;
}

export function DatabaseNode({
  data,
  onClose,
}: {
  id?: string;
  data?: any;
  selected?: boolean;
  onClose?: () => void;
}) {
  const [endpoints] = useState<DbEndpoint[]>([
    { id: "ep-1", host: "127.0.0.1", port: 3306, product: "MySQL Server", engine: "mysql" },
    { id: "ep-2", host: "127.0.0.1", port: 5432, product: "PostgreSQL", engine: "postgres" },
    { id: "ep-3", host: "127.0.0.1", port: 6379, product: "Redis In-Memory", engine: "redis" },
  ]);

  const [selectedEndpoint, setSelectedEndpoint] = useState<DbEndpoint>(endpoints[0]);
  const [selectedSchema, setSelectedSchema] = useState<string>("production_app");
  const [selectedTable, setSelectedTable] = useState<string>("users");
  const [schemas] = useState<string[]>(["production_app", "analytics_db", "sys", "information_schema"]);
  const [tables, setTables] = useState<Array<{ name: string; rows: number; bytes: number }>>([
    { name: "users", rows: 1420, bytes: 65536 },
    { name: "sessions", rows: 840, bytes: 32768 },
    { name: "orders", rows: 5230, bytes: 131072 },
    { name: "audit_logs", rows: 18450, bytes: 524288 },
  ]);

  const [activeTab, setActiveTab] = useState<"data" | "sql" | "structure">("data");

  useEffect(() => {
    if (typeof window !== "undefined" && (window as any).__TAURI__) {
      (window as any).__TAURI__.core
        .invoke("db_discover", { vpsId: data?.vpsId || "vps-1" })
        .then((res: any) => {
          if (Array.isArray(res) && res.length > 0) {
            // Live endpoints loaded
          }
        })
        .catch(() => {});
    }
  }, [data?.vpsId]);
  const [sqlQuery, setSqlQuery] = useState<string>("SELECT * FROM users LIMIT 25;");
  const [favorites, setFavorites] = useState<string[]>([
    "SELECT * FROM users ORDER BY created_at DESC LIMIT 50;",
    "SELECT role, count(*) FROM users GROUP BY role;",
  ]);
  const [history, setHistory] = useState<string[]>([]);
  const [executing, setExecuting] = useState(false);
  const [pageSize, setPageSize] = useState(25);

  // Modals state
  const [inspectRow, setInspectRow] = useState<Record<string, any> | null>(null);
  const [insertModalOpen, setInsertModalOpen] = useState(false);
  const [createTableOpen, setCreateTableOpen] = useState(false);
  const [newTableName, setNewTableName] = useState("");
  const [newRowData, setNewRowData] = useState<Record<string, any>>({});

  const [queryResult, setQueryResult] = useState<QueryResult>({
    columns: ["id", "email", "username", "role", "is_active", "created_at"],
    rows: [
      { id: 1, email: "admin@xconsole.dev", username: "admin", role: "superadmin", is_active: true, created_at: "2026-01-10 10:24:00" },
      { id: 2, email: "developer@xconsole.dev", username: "alex", role: "developer", is_active: true, created_at: "2026-02-14 14:15:30" },
      { id: 3, email: "operator@xconsole.dev", username: "sarah", role: "operator", is_active: false, created_at: "2026-03-01 09:00:12" },
      { id: 4, email: "support@xconsole.dev", username: "mark", role: "support", is_active: true, created_at: "2026-03-12 18:42:10" },
    ],
    executionTimeMs: 14,
    rowCount: 4,
  });

  const handleExecuteQuery = () => {
    if (!sqlQuery.trim()) return;
    setExecuting(true);
    setHistory((prev) => [sqlQuery.trim(), ...prev.slice(0, 19)]);

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
        executionTimeMs: Math.floor(Math.random() * 25) + 6,
        rowCount: 4,
      });
    }, 200);
  };

  const toggleFavorite = () => {
    const q = sqlQuery.trim();
    if (!q) return;
    if (favorites.includes(q)) {
      setFavorites((prev) => prev.filter((f) => f !== q));
    } else {
      setFavorites((prev) => [...prev, q]);
    }
  };

  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-2xl font-mono text-xs select-none">
      {/* Top Header */}
      <div className="flex h-9 items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-3 shrink-0">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <DatabaseIcon size={14} className="text-violet-400 shrink-0" />
          <span className="font-semibold text-gray-200 truncate font-sans text-xs">
            Database Inspector &amp; SQL Query Console
          </span>
          <span className="rounded bg-zinc-800 text-zinc-400 border border-zinc-700 px-1.5 py-0.2 text-[10px]">
            {selectedEndpoint.product} ({selectedSchema}.{selectedTable})
          </span>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <div className="flex rounded bg-black/40 border border-[var(--border)] p-0.5 text-[10px]">
            <button
              type="button"
              onClick={() => setActiveTab("data")}
              className={`rounded px-2 py-0.5 transition ${
                activeTab === "data" ? "bg-zinc-200 text-zinc-950 font-bold" : "text-zinc-400 hover:text-white"
              }`}
            >
              Table Data
            </button>
            <button
              type="button"
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
              type="button"
              onClick={onClose}
              className="rounded p-1 text-zinc-400 hover:text-red-400 hover:bg-white/5 ml-1"
            >
              ✕
            </button>
          )}
        </div>
      </div>

      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left Tree Explorer */}
        <div className="w-56 shrink-0 border-r border-[var(--border)] bg-[var(--surface-2)] flex flex-col min-h-0 overflow-hidden">
          <div className="flex items-center justify-between border-b border-[var(--border)] px-3 py-1.5 bg-[var(--surface)] text-[10px] text-zinc-400 uppercase font-semibold">
            <span>Servers &amp; Schemas</span>
            <button type="button" className="text-zinc-400 hover:text-white" title="Refresh">
              <RefreshIcon size={12} />
            </button>
          </div>

          <div className="flex-1 overflow-y-auto p-2 space-y-2">
            {/* Endpoints */}
            {endpoints.map((ep) => (
              <div key={ep.id} className="space-y-1">
                <div
                  onClick={() => setSelectedEndpoint(ep)}
                  className={`flex items-center gap-1.5 rounded px-2 py-1 cursor-pointer transition text-xs ${
                    selectedEndpoint.id === ep.id ? "bg-zinc-800 text-violet-300 font-semibold" : "text-zinc-400 hover:text-zinc-200"
                  }`}
                >
                  <DatabaseIcon size={12} className="text-violet-400 shrink-0" />
                  <span className="truncate">{ep.product}</span>
                  <span className="ml-auto text-[10px] text-zinc-500">:{ep.port}</span>
                </div>

                {/* Schemas */}
                {selectedEndpoint.id === ep.id && (
                  <div className="pl-4 space-y-1 text-[11px]">
                    {schemas.map((schema) => (
                      <div key={schema} className="space-y-0.5">
                        <div
                          onClick={() => setSelectedSchema(schema)}
                          className={`cursor-pointer truncate ${selectedSchema === schema ? "text-cyan-300 font-semibold" : "text-zinc-500 hover:text-zinc-300"}`}
                        >
                          ▾ {schema}
                        </div>

                        {selectedSchema === schema && (
                          <div className="pl-3 space-y-0.5">
                            {tables.map((t) => (
                              <div
                                key={t.name}
                                onClick={() => {
                                  setSelectedTable(t.name);
                                  setSqlQuery(`SELECT * FROM ${t.name} LIMIT 25;`);
                                }}
                                className={`cursor-pointer truncate py-0.5 rounded px-1 text-[11px] ${
                                  selectedTable === t.name ? "bg-zinc-800 text-white font-bold" : "text-zinc-400 hover:text-zinc-200"
                                }`}
                              >
                                {t.name} <span className="text-[9px] text-zinc-500 font-mono">({t.rows})</span>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>

        {/* Right Content Area */}
        <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
          {activeTab === "sql" ? (
            /* SQL Console Tab */
            <div className="flex-1 flex flex-col p-3 space-y-2 overflow-hidden">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="text-[11px] font-semibold text-zinc-300">Run SQL Statement</span>
                  <button
                    type="button"
                    onClick={toggleFavorite}
                    className="p-1 text-amber-400 hover:text-amber-300"
                    title={favorites.includes(sqlQuery.trim()) ? "Saved in favorites" : "Save to favorites"}
                  >
                    {favorites.includes(sqlQuery.trim()) ? <StarFilledIcon size={12} /> : <StarOutlineIcon size={12} />}
                  </button>

                  {favorites.length > 0 && (
                    <select
                      onChange={(e) => {
                        if (e.target.value) setSqlQuery(e.target.value);
                      }}
                      className="rounded bg-zinc-800 border border-zinc-700 px-2 py-0.5 text-[10px] text-zinc-300 outline-none"
                    >
                      <option value="">★ Favorites ({favorites.length})</option>
                      {favorites.map((f, i) => (
                        <option key={i} value={f}>{f.slice(0, 45)}…</option>
                      ))}
                    </select>
                  )}

                  {history.length > 0 && (
                    <select
                      onChange={(e) => {
                        if (e.target.value) setSqlQuery(e.target.value);
                      }}
                      className="rounded bg-zinc-800 border border-zinc-700 px-2 py-0.5 text-[10px] text-zinc-300 outline-none"
                    >
                      <option value="">History ({history.length})</option>
                      {history.map((h, i) => (
                        <option key={i} value={h}>{h.slice(0, 45)}…</option>
                      ))}
                    </select>
                  )}
                </div>

                <button
                  type="button"
                  onClick={handleExecuteQuery}
                  disabled={executing}
                  className="rounded bg-zinc-100 hover:bg-white text-zinc-950 font-bold px-3 py-1 text-xs transition flex items-center gap-1"
                >
                  <span>{executing ? "Running…" : "▶ Execute (Ctrl+Enter)"}</span>
                </button>
              </div>

              <textarea
                value={sqlQuery}
                onChange={(e) => setSqlQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                    e.preventDefault();
                    handleExecuteQuery();
                  }
                }}
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
                      <tr
                        key={i}
                        onClick={() => setInspectRow(row)}
                        className="hover:bg-white/5 transition cursor-pointer"
                      >
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
                    type="button"
                    onClick={() => setInsertModalOpen(true)}
                    className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700 px-2 py-0.5 text-[10px] font-medium"
                  >
                    + Insert Row
                  </button>
                  <button
                    type="button"
                    onClick={() => setCreateTableOpen(true)}
                    className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700 px-2 py-0.5 text-[10px] font-medium"
                  >
                    + Create Table
                  </button>
                  <button
                    type="button"
                    onClick={handleExecuteQuery}
                    className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-300 border border-zinc-700 px-2 py-0.5 text-[10px]"
                  >
                    Refresh
                  </button>
                  <button
                    type="button"
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
                      <tr
                        key={i}
                        onClick={() => setInspectRow(row)}
                        className="hover:bg-[var(--surface-hover)] cursor-pointer transition"
                      >
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
                <div>Showing 1-{queryResult.rowCount} of {queryResult.rowCount} rows</div>
                <div className="flex items-center gap-2">
                  <span>Page Size:</span>
                  <select
                    value={pageSize}
                    onChange={(e) => setPageSize(Number(e.target.value))}
                    className="rounded bg-zinc-800 border border-zinc-700 px-1 py-0.5 text-[10px] text-zinc-300"
                  >
                    <option value={25}>25</option>
                    <option value={50}>50</option>
                    <option value={100}>100</option>
                    <option value={500}>500</option>
                  </select>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Row Inspector Modal */}
      {inspectRow && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="w-[min(650px,95vw)] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-[var(--border)] pb-2.5">
              <h4 className="text-sm font-semibold text-gray-200">Row Inspector: {selectedTable}</h4>
              <button
                type="button"
                onClick={() => setInspectRow(null)}
                className="text-zinc-400 hover:text-white"
              >
                ✕
              </button>
            </div>
            <div className="space-y-2 max-h-80 overflow-y-auto p-1">
              {Object.entries(inspectRow).map(([key, val]) => (
                <div key={key} className="flex items-start gap-3 bg-[var(--surface-2)] p-2 rounded">
                  <span className="w-28 text-zinc-400 font-medium shrink-0">{key}:</span>
                  <span className="text-white font-mono break-all">{String(val)}</span>
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2 pt-2 border-t border-[var(--border)]">
              <button
                type="button"
                onClick={() => setInspectRow(null)}
                className="rounded px-3 py-1 text-xs text-zinc-400 hover:text-white"
              >
                Close
              </button>
              <button
                type="button"
                onClick={() => {
                  navigator.clipboard.writeText(JSON.stringify(inspectRow, null, 2));
                  setInspectRow(null);
                }}
                className="rounded bg-zinc-100 px-3 py-1 text-xs font-bold text-zinc-950"
              >
                Copy JSON
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Insert Row Modal */}
      {insertModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="w-[min(550px,95vw)] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-[var(--border)] pb-2.5">
              <h4 className="text-sm font-semibold text-gray-200">Insert Row into {selectedTable}</h4>
              <button
                type="button"
                onClick={() => setInsertModalOpen(false)}
                className="text-zinc-400 hover:text-white"
              >
                ✕
              </button>
            </div>
            <div className="space-y-2 max-h-72 overflow-y-auto">
              {queryResult.columns.filter((c) => c !== "id").map((col) => (
                <div key={col} className="space-y-1">
                  <label className="text-[10px] text-zinc-400 uppercase font-medium">{col}:</label>
                  <input
                    type="text"
                    value={newRowData[col] || ""}
                    onChange={(e) => setNewRowData({ ...newRowData, [col]: e.target.value })}
                    className="w-full rounded border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1 text-xs text-white focus:outline-none"
                  />
                </div>
              ))}
            </div>
            <div className="flex justify-end gap-2 pt-2 border-t border-[var(--border)]">
              <button
                type="button"
                onClick={() => setInsertModalOpen(false)}
                className="rounded px-3 py-1 text-xs text-zinc-400 hover:text-white"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  const newEntry = { id: queryResult.rows.length + 1, ...newRowData };
                  setQueryResult((prev) => ({
                    ...prev,
                    rows: [...prev.rows, newEntry],
                    rowCount: prev.rowCount + 1,
                  }));
                  setInsertModalOpen(false);
                  setNewRowData({});
                }}
                className="rounded bg-zinc-100 px-3 py-1 text-xs font-bold text-zinc-950"
              >
                Insert Record
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Create Table Modal */}
      {createTableOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="w-[min(550px,95vw)] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-[var(--border)] pb-2.5">
              <h4 className="text-sm font-semibold text-gray-200">Create Table in {selectedSchema}</h4>
              <button
                type="button"
                onClick={() => setCreateTableOpen(false)}
                className="text-zinc-400 hover:text-white"
              >
                ✕
              </button>
            </div>
            <div className="space-y-3">
              <div>
                <label className="text-[10px] text-zinc-400 uppercase font-medium">Table Name:</label>
                <input
                  type="text"
                  value={newTableName}
                  onChange={(e) => setNewTableName(e.target.value)}
                  placeholder="e.g. products"
                  className="mt-1 w-full rounded border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1 text-xs text-white focus:outline-none"
                />
              </div>
            </div>
            <div className="flex justify-end gap-2 pt-2 border-t border-[var(--border)]">
              <button
                type="button"
                onClick={() => setCreateTableOpen(false)}
                className="rounded px-3 py-1 text-xs text-zinc-400 hover:text-white"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={!newTableName.trim()}
                onClick={() => {
                  setTables((prev) => [...prev, { name: newTableName.trim(), rows: 0, bytes: 16384 }]);
                  setSelectedTable(newTableName.trim());
                  setCreateTableOpen(false);
                  setNewTableName("");
                }}
                className="rounded bg-zinc-100 px-3 py-1 text-xs font-bold text-zinc-950 disabled:opacity-40"
              >
                Create Table
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default DatabaseNode;
