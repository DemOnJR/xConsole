import { useCallback, useEffect, useRef, useState } from "react";
import { NodeResizer, useStore, type NodeProps } from "@xyflow/react";
import { api, type DbColumn, type DbResultSet, type DbRowKey } from "../lib/tauri";
import { useCanvasStore, type DbNode as DbNodeType } from "../stores/canvasStore";
import { CodeEditArea } from "./CodeEditArea";
import { DatabaseTree, newInstance, type DbInstance } from "./DatabaseTree";
import { DatabaseIcon } from "./icons";

const PAGE_SIZE = 200;

type Tab = "data" | "structure" | "sql";

/** Which table the right-hand pane is showing. */
interface Selection {
  endpointId: string;
  sessionId: string;
  schema: string;
  table: string;
}

/** Grid for table data and query results. Editable when the rows have a primary key. */
function Grid({
  set,
  columns,
  onEdit,
}: {
  set: DbResultSet;
  columns?: DbColumn[];
  onEdit?: (rowIndex: number, column: string, next: string | null) => void;
}) {
  const [editing, setEditing] = useState<{ row: number; col: number } | null>(null);
  const [draft, setDraft] = useState("");

  const hasKey = (columns ?? []).some((c) => c.primary);
  const editable = Boolean(onEdit) && hasKey;

  if (set.columns.length === 0) {
    return (
      <p className="p-3 text-[11px] text-gray-500">
        {set.affected != null
          ? `${set.affected} row${set.affected === 1 ? "" : "s"} affected.`
          : "Statement ran. No rows returned."}
      </p>
    );
  }

  return (
    <div className="h-full overflow-auto">
      <table className="w-full border-collapse text-left font-mono text-[11px]">
        <thead className="sticky top-0 bg-[var(--surface)]">
          <tr>
            {set.columns.map((c) => {
              const meta = columns?.find((m) => m.name === c);
              return (
                <th
                  key={c}
                  className="whitespace-nowrap border-b border-r border-[var(--border)] px-2 py-1 font-medium text-gray-300 last:border-r-0"
                  title={meta ? `${meta.data_type}${meta.primary ? " · primary key" : ""}` : c}
                >
                  {meta?.primary ? <span className="text-amber-400">🔑 </span> : null}
                  {c}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {set.rows.map((row, ri) => (
            <tr key={ri} className="hover:bg-[var(--border)]/40">
              {row.map((cell, ci) => {
                const isEditing = editing?.row === ri && editing?.col === ci;
                return (
                  <td
                    key={ci}
                    className="max-w-[320px] truncate border-b border-r border-[var(--border)] px-2 py-0.5 text-gray-300 last:border-r-0"
                    title={cell ?? "NULL"}
                    onDoubleClick={() => {
                      if (!editable) return;
                      setEditing({ row: ri, col: ci });
                      setDraft(cell ?? "");
                    }}
                  >
                    {isEditing ? (
                      <input
                        autoFocus
                        value={draft}
                        onChange={(e) => setDraft(e.target.value)}
                        onBlur={() => setEditing(null)}
                        onKeyDown={(e) => {
                          if (e.key === "Escape") setEditing(null);
                          if (e.key === "Enter") {
                            onEdit?.(ri, set.columns[ci], draft);
                            setEditing(null);
                          }
                        }}
                        className="w-full bg-[var(--bg)] px-1 text-[11px] text-gray-100 outline-none"
                      />
                    ) : cell === null ? (
                      <span className="italic text-gray-600">NULL</span>
                    ) : (
                      cell
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      {set.rows.length === 0 ? <p className="p-3 text-[11px] text-gray-500">No rows.</p> : null}
    </div>
  );
}

/**
 * A database browser for one server.
 *
 * Everything reaches the databases **through the existing SSH connection** — queries run
 * via the host's own `mysql` client over an exec channel, and a container is reached with
 * `docker exec`. Nothing needs port 3306 open to the internet, and a container does not
 * need a published port.
 *
 * The tree lists every instance found on the host at once — native installs and Docker
 * containers, named — rather than making the user pick one up front, because "what
 * databases are on this box" is the question you actually have. Credentials are per
 * instance, since a host install and a container rarely share a password.
 */
export function DatabaseNode({ id, data, selected }: NodeProps<DbNodeType>) {
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);

  const [instances, setInstances] = useState<DbInstance[]>([]);
  const [scanning, setScanning] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [sel, setSel] = useState<Selection | null>(null);
  const [columns, setColumns] = useState<DbColumn[]>([]);
  const [rows, setRows] = useState<DbResultSet | null>(null);
  const [page, setPage] = useState(0);
  const [tab, setTab] = useState<Tab>("data");
  const [sql, setSql] = useState("SELECT * FROM ");
  const [sqlResult, setSqlResult] = useState<DbResultSet | null>(null);
  const [busy, setBusy] = useState(false);

  // Every session opened by this node, so unmount can close all of them. A ref because
  // the cleanup must see the latest set without re-running on every change.
  const sessionsRef = useRef<Set<string>>(new Set());

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      // Discovery and remembered logins together, so an instance appears with its saved
      // credential already attached rather than briefly offering an empty form.
      const [found, saved] = await Promise.all([
        api.dbDiscover(data.vpsId),
        api.dbListConnections(data.vpsId).catch(() => []),
      ]);
      const savedByEndpoint = new Map(saved.map((s) => [s.endpoint_id, s]));
      setInstances((prev) => {
        // Keep live sessions across a rescan rather than making the user sign in again.
        const byId = new Map(prev.map((i) => [i.endpoint.id, i]));
        return found.map((ep) => {
          const existing = byId.get(ep.id);
          const base = existing ? { ...existing, endpoint: ep } : newInstance(ep);
          return { ...base, saved: savedByEndpoint.get(ep.id) };
        });
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }, [data.vpsId]);

  /** Re-read just the saved logins, after one is added or forgotten. */
  const refreshSaved = useCallback(async () => {
    try {
      const saved = await api.dbListConnections(data.vpsId);
      const byEndpoint = new Map(saved.map((s) => [s.endpoint_id, s]));
      setInstances((prev) =>
        prev.map((i) => ({ ...i, saved: byEndpoint.get(i.endpoint.id) })),
      );
    } catch {
      // Non-fatal: the tree still works, it just won't show the saved login yet.
    }
  }, [data.vpsId]);

  const forgetSaved = useCallback(
    async (id: string) => {
      try {
        await api.dbForgetConnection(id);
        await refreshSaved();
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshSaved],
  );

  useEffect(() => {
    void scan();
  }, [scan]);

  useEffect(
    () => () => {
      for (const sid of sessionsRef.current) void api.dbDisconnect(sid).catch(() => {});
    },
    [],
  );

  const patch = useCallback((endpointId: string, p: Partial<DbInstance>) => {
    if (p.sessionId) sessionsRef.current.add(p.sessionId);
    setInstances((prev) =>
      prev.map((i) => (i.endpoint.id === endpointId ? { ...i, ...p } : i)),
    );
  }, []);

  const openTable = useCallback(
    async (next: Selection, atPage = 0) => {
      setSel(next);
      setPage(atPage);
      setBusy(true);
      setError(null);
      try {
        const [cols, data] = await Promise.all([
          api.dbDescribeTable(next.sessionId, next.schema, next.table),
          api.dbSelectPage(next.sessionId, next.schema, next.table, PAGE_SIZE, atPage * PAGE_SIZE),
        ]);
        setColumns(cols);
        setRows(data);
        setTab("data");
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  /** Identify a row by its primary key, so an edit can never touch more than one. */
  const rowKey = (rowIndex: number): DbRowKey | null => {
    if (!rows) return null;
    const pk = columns.filter((c) => c.primary);
    if (pk.length === 0) return null;
    const key: DbRowKey = [];
    for (const col of pk) {
      const ci = rows.columns.indexOf(col.name);
      if (ci === -1) return null;
      key.push([col.name, rows.rows[rowIndex][ci]]);
    }
    return key;
  };

  const editCell = async (rowIndex: number, column: string, next: string | null) => {
    const key = rowKey(rowIndex);
    if (!sel || !key) {
      setError("This table has no primary key, so a single row can't be edited safely.");
      return;
    }
    try {
      await api.dbUpdateCell(sel.sessionId, sel.schema, sel.table, column, next, key);
      await openTable(sel, page);
    } catch (e) {
      setError(String(e));
    }
  };

  const runSql = async () => {
    if (!sel?.sessionId || !sql.trim()) {
      setError("Open a table first, so the query knows which server to run against.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setSqlResult(await api.dbRunSql(sel.sessionId, sql));
    } catch (e) {
      setError(String(e));
      setSqlResult(null);
    } finally {
      setBusy(false);
    }
  };

  const connectedCount = instances.filter((i) => i.sessionId).length;

  return (
    <div
      className={`flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-violet-500" : "border-[var(--border)]"}`}
      onMouseDown={() => focus(id)}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
    >
      <NodeResizer
        minWidth={520}
        minHeight={280}
        isVisible={selected}
        lineClassName="border-violet-500"
        handleClassName="h-2 w-2 rounded bg-violet-500"
      />

      <div
        className="flex shrink-0 cursor-move items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-2 py-1.5"
        onDoubleClick={() => focus(id)}
      >
        <DatabaseIcon size={13} className="shrink-0 text-violet-400" />
        <span className="truncate text-xs font-medium text-gray-200">{data.name}</span>
        <span className="shrink-0 text-[10px] text-gray-600">
          {instances.length} instance{instances.length === 1 ? "" : "s"}
          {connectedCount > 0 ? ` · ${connectedCount} connected` : ""}
        </span>
        {sel ? (
          <span className="truncate text-[10px] text-violet-300">
            {sel.schema}.{sel.table}
          </span>
        ) : null}
        <button
          className="ml-auto shrink-0 rounded px-1 text-gray-500 hover:bg-[var(--border)] hover:text-white"
          onClick={() => removeNode(id)}
          data-tooltip="Close"
        >
          ✕
        </button>
      </div>

      <div className="nodrag nowheel flex min-h-0 flex-1 flex-col">
        {error ? (
          <div className="shrink-0 border-b border-red-500/40 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
            {error}
          </div>
        ) : null}

        <div className="flex min-h-0 flex-1">
          <DatabaseTree
            instances={instances}
            vpsId={data.vpsId}
            scanning={scanning}
            selected={sel}
            onPatch={patch}
            onSelectTable={(inst, schema, table) => {
              if (!inst.sessionId) return;
              void openTable({
                endpointId: inst.endpoint.id,
                sessionId: inst.sessionId,
                schema,
                table,
              });
            }}
            onRescan={() => void scan()}
            onSavedChanged={() => void refreshSaved()}
            onForget={(id) => void forgetSaved(id)}
          />

          <div className="flex min-w-0 flex-1 flex-col">
            <div className="flex shrink-0 items-center gap-1 border-b border-[var(--border)] px-2 py-1">
              {(["data", "structure", "sql"] as Tab[]).map((t) => (
                <button
                  key={t}
                  onClick={() => setTab(t)}
                  className={`rounded px-2 py-0.5 text-[11px] capitalize ${
                    tab === t ? "bg-violet-600 text-white" : "text-gray-400 hover:bg-[var(--border)]"
                  }`}
                >
                  {t}
                </button>
              ))}
              {tab === "data" && sel ? (
                <div className="ml-auto flex items-center gap-1 text-[10px] text-gray-500">
                  <button
                    disabled={page === 0}
                    onClick={() => void openTable(sel, page - 1)}
                    className="rounded px-1 hover:bg-[var(--border)] disabled:opacity-30"
                  >
                    ‹
                  </button>
                  <span className="tabular-nums">
                    {page * PAGE_SIZE + 1}–{page * PAGE_SIZE + (rows?.rows.length ?? 0)}
                  </span>
                  <button
                    disabled={(rows?.rows.length ?? 0) < PAGE_SIZE}
                    onClick={() => void openTable(sel, page + 1)}
                    className="rounded px-1 hover:bg-[var(--border)] disabled:opacity-30"
                  >
                    ›
                  </button>
                </div>
              ) : null}
            </div>

            <div className="min-h-0 flex-1 overflow-hidden">
              {tab === "data" ? (
                rows ? (
                  <Grid set={rows} columns={columns} onEdit={(r, c, v) => void editCell(r, c, v)} />
                ) : (
                  <p className="p-3 text-[11px] text-gray-500">
                    {busy
                      ? "Loading…"
                      : "Expand a server on the left, sign in, then pick a table."}
                  </p>
                )
              ) : null}

              {tab === "structure" ? (
                columns.length > 0 ? (
                  <Grid
                    set={{
                      columns: ["Column", "Type", "Null", "Key", "Default", "Extra"],
                      rows: columns.map((c) => [
                        c.name,
                        c.data_type,
                        c.nullable ? "YES" : "NO",
                        c.primary ? "PRI" : "",
                        c.default,
                        c.extra,
                      ]),
                      affected: null,
                      message: null,
                    }}
                  />
                ) : (
                  <p className="p-3 text-[11px] text-gray-500">Pick a table on the left.</p>
                )
              ) : null}

              {tab === "sql" ? (
                <div className="flex h-full flex-col">
                  <div className="h-1/2 min-h-0 p-1">
                    <CodeEditArea value={sql} onChange={setSql} path="query.sql" />
                  </div>
                  <div className="flex shrink-0 items-center gap-2 border-y border-[var(--border)] px-2 py-1">
                    <button
                      onClick={() => void runSql()}
                      disabled={busy}
                      className="rounded bg-violet-600 px-2 py-0.5 text-[11px] text-white hover:bg-violet-500 disabled:opacity-50"
                    >
                      {busy ? "Running…" : "Run"}
                    </button>
                    <span className="truncate text-[10px] text-gray-600">
                      {sel ? `against ${sel.schema} on ${sel.endpointId}` : "open a table first"}
                    </span>
                    {sqlResult?.message ? (
                      <span className="truncate text-[10px] text-amber-300">
                        {sqlResult.message}
                      </span>
                    ) : null}
                  </div>
                  <div className="min-h-0 flex-1 overflow-hidden">
                    {sqlResult ? (
                      <Grid set={sqlResult} />
                    ) : (
                      <p className="p-3 text-[11px] text-gray-500">
                        Write a statement and press Run.
                      </p>
                    )}
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
