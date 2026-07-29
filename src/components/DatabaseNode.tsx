import { useCallback, useEffect, useRef, useState } from "react";
import {
  NodeResizer,
  useStore,
  type NodeProps,
} from "@xyflow/react";
import {
  api,
  type DbColumn,
  type DbEndpoint,
  type DbResultSet,
  type DbRowKey,
  type DbTable,
} from "../lib/tauri";
import { useCanvasStore, type DbNode as DbNodeType } from "../stores/canvasStore";
import { CodeEditArea } from "./CodeEditArea";
import { DatabaseIcon } from "./icons";

const PAGE_SIZE = 200;

type Phase = "picking" | "credentials" | "connecting" | "ready" | "error";
type Tab = "data" | "structure" | "sql";

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** Read-only grid used for query results and table data. */
function Grid({
  set,
  columns,
  onEdit,
}: {
  set: DbResultSet;
  /** Column metadata, when the rows came from a real table (enables editing). */
  columns?: DbColumn[];
  onEdit?: (rowIndex: number, column: string, next: string | null) => void;
}) {
  const [editing, setEditing] = useState<{ row: number; col: number } | null>(null);
  const [draft, setDraft] = useState("");

  const primaryCols = columns?.filter((c) => c.primary).map((c) => c.name) ?? [];
  const editable = Boolean(onEdit) && primaryCols.length > 0;

  if (set.columns.length === 0) {
    return (
      <p className="p-3 text-[11px] text-gray-500">
        {set.affected != null
          ? `${set.affected} row${set.affected === 1 ? "" : "s"} affected.`
          : "No results."}
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
      {set.rows.length === 0 ? (
        <p className="p-3 text-[11px] text-gray-500">No rows.</p>
      ) : null}
    </div>
  );
}

/**
 * A database browser on the canvas: pick a server the host is running, connect, then
 * browse schemas and tables, edit cells, and run SQL.
 *
 * Connecting is deliberately a two-step flow — discover what is actually listening on
 * the host (including containers that publish nothing), then ask for credentials for the
 * one chosen. Typing a host and port by hand is the thing this is meant to replace.
 */
export function DatabaseNode({ id, data, selected }: NodeProps<DbNodeType>) {
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);

  const [phase, setPhase] = useState<Phase>("picking");
  const [error, setError] = useState<string | null>(null);
  const [endpoints, setEndpoints] = useState<DbEndpoint[]>([]);
  const [chosen, setChosen] = useState<DbEndpoint | null>(null);
  const [user, setUser] = useState("root");
  const [password, setPassword] = useState("");
  const [version, setVersion] = useState("");

  const sessionRef = useRef<string | null>(null);
  const [schemas, setSchemas] = useState<string[]>([]);
  const [schema, setSchema] = useState<string | null>(null);
  const [tables, setTables] = useState<DbTable[]>([]);
  const [table, setTable] = useState<string | null>(null);
  const [columns, setColumns] = useState<DbColumn[]>([]);
  const [rows, setRows] = useState<DbResultSet | null>(null);
  const [page, setPage] = useState(0);
  const [tab, setTab] = useState<Tab>("data");
  const [sql, setSql] = useState("SELECT * FROM ");
  const [sqlResult, setSqlResult] = useState<DbResultSet | null>(null);
  const [busy, setBusy] = useState(false);

  // Discover on mount — the whole point is not having to know what's installed.
  useEffect(() => {
    let alive = true;
    setBusy(true);
    api
      .dbDiscover(data.vpsId)
      .then((found) => {
        if (!alive) return;
        setEndpoints(found);
        if (found.length === 1) {
          setChosen(found[0]);
          setPhase("credentials");
        } else if (found.length === 0) {
          setError("No MySQL or MariaDB server found on this host.");
          setPhase("error");
        }
      })
      .catch((e) => {
        if (!alive) return;
        setError(String(e));
        setPhase("error");
      })
      .finally(() => alive && setBusy(false));
    return () => {
      alive = false;
    };
  }, [data.vpsId]);

  // Close the connection when the node goes away.
  useEffect(
    () => () => {
      const sid = sessionRef.current;
      if (sid) void api.dbDisconnect(sid).catch(() => {});
    },
    [],
  );

  const connect = async () => {
    if (!chosen) return;
    setPhase("connecting");
    setError(null);
    try {
      const res = await api.dbConnect({
        vps_id: data.vpsId,
        container: chosen.container,
        host: chosen.host,
        port: chosen.port,
        user,
        password,
        database: null,
      });
      sessionRef.current = res.session_id;
      setVersion(res.version);
      // Don't keep the password in component state any longer than the call needs it;
      // the backend holds it for the session.
      setPassword("");
      const dbs = await api.dbListDatabases(res.session_id);
      setSchemas(dbs);
      setPhase("ready");
    } catch (e) {
      setError(String(e));
      setPhase("credentials");
    }
  };

  const openSchema = useCallback(async (name: string) => {
    const sid = sessionRef.current;
    if (!sid) return;
    setSchema(name);
    setTable(null);
    setRows(null);
    try {
      await api.dbUseDatabase(sid, name);
      setTables(await api.dbListTables(sid, name));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const openTable = useCallback(
    async (name: string, atPage = 0) => {
      const sid = sessionRef.current;
      if (!sid || !schema) return;
      setTable(name);
      setPage(atPage);
      setBusy(true);
      try {
        const [cols, data] = await Promise.all([
          api.dbDescribeTable(sid, schema, name),
          api.dbSelectPage(sid, schema, name, PAGE_SIZE, atPage * PAGE_SIZE),
        ]);
        setColumns(cols);
        setRows(data);
        setTab("data");
        setError(null);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [schema],
  );

  /** Build the primary-key identification for a row in the current grid. */
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
    const sid = sessionRef.current;
    const key = rowKey(rowIndex);
    if (!sid || !schema || !table || !key) {
      setError("This table has no primary key, so a single row can't be edited safely.");
      return;
    }
    try {
      await api.dbUpdateCell(sid, schema, table, column, next, key);
      await openTable(table, page);
    } catch (e) {
      setError(String(e));
    }
  };

  const runSql = async () => {
    const sid = sessionRef.current;
    if (!sid || !sql.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setSqlResult(await api.dbRunSql(sid, sql));
    } catch (e) {
      setError(String(e));
      setSqlResult(null);
    } finally {
      setBusy(false);
    }
  };

  const header = (
    <div
      className="flex shrink-0 cursor-move items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-2 py-1.5"
      onDoubleClick={() => focus(id)}
    >
      <DatabaseIcon size={13} className="shrink-0 text-violet-400" />
      <span className="truncate text-xs font-medium text-gray-200">{data.name}</span>
      {version ? (
        <span className="truncate font-mono text-[10px] text-gray-500">{version}</span>
      ) : null}
      {schema ? (
        <span className="truncate text-[10px] text-violet-300">{schema}</span>
      ) : null}
      <button
        className="ml-auto shrink-0 rounded px-1 text-gray-500 hover:bg-[var(--border)] hover:text-white"
        onClick={() => removeNode(id)}
        data-tooltip="Close"
      >
        ✕
      </button>
    </div>
  );

  return (
    <div
      className={`flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-violet-500" : "border-[var(--border)]"}`}
      onMouseDown={() => focus(id)}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
    >
      <NodeResizer
        minWidth={420}
        minHeight={260}
        isVisible={selected}
        lineClassName="border-violet-500"
        handleClassName="h-2 w-2 rounded bg-violet-500"
      />
      {header}

      <div className="nodrag nowheel flex min-h-0 flex-1 flex-col">
        {error ? (
          <div className="shrink-0 border-b border-red-500/40 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
            {error}
          </div>
        ) : null}

        {phase === "picking" ? (
          <div className="flex-1 overflow-auto p-3">
            <p className="mb-2 text-[11px] text-gray-400">
              {busy ? "Looking for databases on this host…" : "Pick a database server:"}
            </p>
            {endpoints.map((e) => (
              <button
                key={e.id}
                onClick={() => {
                  setChosen(e);
                  setPhase("credentials");
                }}
                className="mb-1 block w-full rounded border border-[var(--border)] px-2 py-1.5 text-left text-[11px] text-gray-200 hover:bg-[var(--border)]"
              >
                <span className="text-violet-300">
                  {e.kind === "docker" ? "🐳" : "🖥"}
                </span>{" "}
                {e.label}
                <span className="ml-1 font-mono text-[10px] text-gray-500">
                  {e.host}:{e.port}
                </span>
              </button>
            ))}
          </div>
        ) : null}

        {phase === "credentials" || phase === "connecting" ? (
          <form
            className="flex-1 space-y-2 p-3"
            onSubmit={(e) => {
              e.preventDefault();
              void connect();
            }}
          >
            <p className="text-[11px] text-gray-400">
              Sign in to <span className="text-violet-300">{chosen?.label}</span>
            </p>
            <input
              value={user}
              onChange={(e) => setUser(e.target.value)}
              placeholder="User"
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-xs text-gray-100 outline-none focus:border-violet-500"
            />
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Password"
              className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-xs text-gray-100 outline-none focus:border-violet-500"
            />
            <div className="flex gap-2">
              <button
                type="submit"
                disabled={phase === "connecting"}
                className="rounded bg-violet-600 px-2 py-1 text-xs text-white hover:bg-violet-500 disabled:opacity-50"
              >
                {phase === "connecting" ? "Connecting…" : "Connect"}
              </button>
              {endpoints.length > 1 ? (
                <button
                  type="button"
                  onClick={() => setPhase("picking")}
                  className="rounded border border-[var(--border)] px-2 py-1 text-xs text-gray-300 hover:bg-[var(--border)]"
                >
                  Back
                </button>
              ) : null}
            </div>
            <p className="text-[10px] text-gray-600">
              The password is held only for this connection and never written to disk.
            </p>
          </form>
        ) : null}

        {phase === "ready" ? (
          <div className="flex min-h-0 flex-1">
            {/* Schema / table tree */}
            <div className="w-48 shrink-0 overflow-auto border-r border-[var(--border)] bg-[var(--surface-2)] py-1">
              {schemas.map((s) => (
                <div key={s}>
                  <button
                    onClick={() => void openSchema(s)}
                    className={`block w-full truncate px-2 py-0.5 text-left text-[11px] ${
                      schema === s ? "bg-violet-600/25 text-violet-200" : "text-gray-300 hover:bg-[var(--border)]"
                    }`}
                  >
                    {schema === s ? "▾" : "▸"} {s}
                  </button>
                  {schema === s
                    ? tables.map((t) => (
                        <button
                          key={t.name}
                          onClick={() => void openTable(t.name)}
                          className={`block w-full truncate py-0.5 pl-5 pr-2 text-left text-[11px] ${
                            table === t.name
                              ? "bg-violet-600/20 text-violet-200"
                              : "text-gray-400 hover:bg-[var(--border)]"
                          }`}
                          title={`${t.rows} rows · ${formatBytes(t.bytes)} · ${t.engine}`}
                        >
                          {t.name}
                        </button>
                      ))
                    : null}
                </div>
              ))}
            </div>

            {/* Right pane */}
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
                {tab === "data" && table ? (
                  <div className="ml-auto flex items-center gap-1 text-[10px] text-gray-500">
                    <button
                      disabled={page === 0}
                      onClick={() => void openTable(table, page - 1)}
                      className="rounded px-1 hover:bg-[var(--border)] disabled:opacity-30"
                    >
                      ‹
                    </button>
                    <span className="tabular-nums">
                      {page * PAGE_SIZE + 1}–{page * PAGE_SIZE + (rows?.rows.length ?? 0)}
                    </span>
                    <button
                      disabled={(rows?.rows.length ?? 0) < PAGE_SIZE}
                      onClick={() => void openTable(table, page + 1)}
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
                      {busy ? "Loading…" : "Pick a table on the left."}
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
        ) : null}
      </div>
    </div>
  );
}
