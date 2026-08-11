import { useCallback, useEffect, useRef, useState } from "react";
import { NodeResizer, useStore, type NodeProps } from "@xyflow/react";
import { api, type DbColumn, type DbResultSet, type DbRowKey } from "../lib/tauri";
import { useCanvasStore, type DbNode as DbNodeType } from "../stores/canvasStore";
import { useMouseNavButtons, useNavHistory } from "../hooks/useNavHistory";
import { dialog } from "../stores/dialogStore";
import { CodeEditArea } from "./CodeEditArea";
import { DatabaseTree, newInstance, type DbInstance } from "./DatabaseTree";
import { DatabaseIcon } from "./icons";

const PAGE_SIZE = 200;

/**
 * Split a SQL dump into statements for sequential import.
 * Handles `--` line comments and `/* … *​/` block comments; does not fully parse
 * string literals (good enough for typical mysqldump / pg_dump style files).
 */
function splitSqlStatements(script: string): string[] {
  // Strip block comments then line comments.
  const stripped = script
    .replace(/\/\*[\s\S]*?\*\//g, "\n")
    .replace(/^[ \t]*--[^\n]*$/gm, "");
  const parts = stripped
    .split(/;\s*(?:\r?\n|$)/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0 && !/^\/\*/.test(s));
  if (parts.length === 0) {
    const t = script.trim();
    return t ? [t] : [];
  }
  // Re-attach semicolon is not required by most engines; keep bare statements.
  return parts;
}

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
  onDeleteRows,
}: {
  set: DbResultSet;
  columns?: DbColumn[];
  onEdit?: (rowIndex: number, column: string, next: string | null) => void;
  /** Delete the given row indices. Absent for result sets that aren't a real table. */
  onDeleteRows?: (rowIndices: number[]) => void;
}) {
  const [editing, setEditing] = useState<{ row: number; col: number } | null>(null);
  const [draft, setDraft] = useState("");
  const [selected, setSelected] = useState<Set<number>>(() => new Set());
  const lastClicked = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const hasKey = (columns ?? []).some((c) => c.primary);
  const editable = Boolean(onEdit) && hasKey;
  /** Row selection is always on so users can copy JSON/CSV; delete only with a PK. */
  const selectable = true;
  const canDelete = Boolean(onDeleteRows) && hasKey;

  // A new result set invalidates the old indices — keeping them would delete whatever
  // now happens to sit at those positions.
  useEffect(() => {
    setSelected(new Set());
    lastClicked.current = null;
  }, [set]);

  /** Click, ctrl-click to toggle, shift-click for a range — as a file list behaves. */
  const toggleRow = (index: number, e: React.MouseEvent) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (e.shiftKey && lastClicked.current !== null) {
        const [from, to] = [lastClicked.current, index].sort((a, b) => a - b);
        for (let i = from; i <= to; i += 1) next.add(i);
      } else if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
    lastClicked.current = index;
  };

  const allSelected = set.rows.length > 0 && selected.size === set.rows.length;

  // Ctrl+wheel scrolls sideways. A wide table is the normal case here and reaching for
  // the horizontal scrollbar is tedious; the browser would otherwise treat ctrl+wheel as
  // page zoom, hence preventDefault. Registered non-passively because a passive listener
  // is not allowed to preventDefault.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY !== 0 ? e.deltaY : e.deltaX;
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

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
    <div className="flex h-full min-h-0 flex-col">
      {selected.size > 0 ? (
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border)] bg-violet-950/30 px-2 py-1 text-[11px]">
          <span className="text-violet-200">
            {selected.size} row{selected.size === 1 ? "" : "s"} selected
          </span>
          <button
            onClick={() => setSelected(new Set())}
            className="text-gray-400 hover:text-gray-200"
          >
            Clear
          </button>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--border)]"
            data-tooltip="Copy selected rows as JSON"
            onClick={() => {
              const idxs = [...selected].sort((a, b) => a - b);
              const objs = idxs.map((i) => {
                const row = set.rows[i];
                const o: Record<string, string | null> = {};
                set.columns.forEach((c, ci) => {
                  o[c] = row[ci] ?? null;
                });
                return o;
              });
              void navigator.clipboard.writeText(JSON.stringify(objs, null, 2));
            }}
          >
            Copy JSON
          </button>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--border)]"
            data-tooltip="Copy selected rows as CSV"
            onClick={() => {
              const idxs = [...selected].sort((a, b) => a - b);
              const esc = (v: string | null) => {
                const s = v ?? "";
                if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
                return s;
              };
              const lines = [
                set.columns.map(esc).join(","),
                ...idxs.map((i) => set.rows[i].map(esc).join(",")),
              ];
              void navigator.clipboard.writeText(lines.join("\n"));
            }}
          >
            Copy CSV
          </button>
          {canDelete ? (
            <button
              onClick={() => onDeleteRows?.([...selected].sort((a, b) => a - b))}
              className="ml-auto rounded bg-red-700 px-2 py-0.5 text-white hover:bg-red-600"
            >
              Delete selected
            </button>
          ) : null}
        </div>
      ) : null}

      {/* The scroll container. `overflow-auto` with a bounded height is what makes
          vertical scrolling work; the table below is sized to its content (not w-full)
          so it can exceed this box and scroll horizontally too. */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto">
        <table className="w-max min-w-full border-collapse text-left font-mono text-[11px]">
        <thead className="sticky top-0 z-10 bg-[var(--surface)]">
          <tr>
            {selectable ? (
              <th className="sticky left-0 z-20 w-6 border-b border-r border-[var(--border)] bg-[var(--surface)] px-1 py-1">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={() =>
                    setSelected(
                      allSelected ? new Set() : new Set(set.rows.map((_, i) => i)),
                    )
                  }
                  data-tooltip="Select all on this page"
                />
              </th>
            ) : null}
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
          {set.rows.map((row, ri) => {
            const isSelected = selected.has(ri);
            return (
              <tr
                key={ri}
                className={isSelected ? "bg-violet-600/25" : "hover:bg-[var(--border)]/40"}
              >
                {selectable ? (
                  <td
                    className={`sticky left-0 z-10 border-b border-r border-[var(--border)] px-1 ${
                      isSelected ? "bg-violet-900/60" : "bg-[var(--bg)]"
                    }`}
                    onClick={(e) => toggleRow(ri, e)}
                  >
                    <input type="checkbox" checked={isSelected} readOnly tabIndex={-1} />
                  </td>
                ) : null}
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
            );
          })}
        </tbody>
        </table>
        {set.rows.length === 0 ? (
          <p className="p-3 text-[11px] text-gray-500">No rows.</p>
        ) : null}
      </div>
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
  const [sqlHistory, setSqlHistory] = useState<string[]>(() => {
    try {
      return JSON.parse(
        localStorage.getItem(`xconsole-sql-history:${data.vpsId}`) || "[]",
      ) as string[];
    } catch {
      return [];
    }
  });
  const favKey = `xconsole-sql-favorites:${data.vpsId}`;
  const [sqlFavorites, setSqlFavorites] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(favKey) || "[]") as string[];
    } catch {
      return [];
    }
  });
  const saveFavorites = (next: string[]) => {
    setSqlFavorites(next);
    try {
      localStorage.setItem(favKey, JSON.stringify(next));
    } catch {
      /* ignore */
    }
  };
  const toggleFavorite = () => {
    const q = sql.trim();
    if (!q) return;
    if (sqlFavorites.includes(q)) {
      saveFavorites(sqlFavorites.filter((f) => f !== q));
    } else {
      saveFavorites([q, ...sqlFavorites].slice(0, 30));
    }
  };

  // Every session opened by this node, so unmount can close all of them. A ref because
  // the cleanup must see the latest set without re-running on every change.
  const sessionsRef = useRef<Set<string>>(new Set());
  /** Scopes the mouse back/forward buttons to this panel. */
  const panelRef = useRef<HTMLDivElement | null>(null);

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

  // No teardown on unmount, by design.
  //
  // A node unmounts whenever something re-renders it out of the tree — the agent panel
  // expanding, a workspace switch — none of which mean the user is finished with their
  // database. Disconnecting there closed every open connection and threw away the
  // browsing state behind it. Terminals and the SFTP browser already keep their sessions;
  // this matches them. `closeNode` is what actually disconnects.

  const patch = useCallback((endpointId: string, p: Partial<DbInstance>) => {
    if (p.sessionId) sessionsRef.current.add(p.sessionId);
    setInstances((prev) =>
      prev.map((i) => (i.endpoint.id === endpointId ? { ...i, ...p } : i)),
    );
  }, []);

  const [tableRowCount, setTableRowCount] = useState<number | null>(null);

  /** Load a table without touching history — used when replaying back/forward. */
  const showTable = useCallback(
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
        // Best-effort exact count for the pager (ignored if engine refuses).
        if (atPage === 0) {
          void api
            .dbRunSql(
              next.sessionId,
              `SELECT COUNT(*) AS c FROM ${next.schema}.${next.table}`,
            )
            .then((r) => {
              const v = r.rows[0]?.[0];
              const n = v != null ? Number(v) : NaN;
              setTableRowCount(Number.isFinite(n) ? n : null);
            })
            .catch(() => setTableRowCount(null));
        }
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  // Back/forward across tables, like the SFTP panel. Paging and post-edit refreshes call
  // showTable directly so they don't pile up history entries for the same table.
  const history = useNavHistory<Selection>({
    current: sel,
    go: useCallback((entry: Selection) => void showTable(entry), [showTable]),
    isSame: (a, b) =>
      a.endpointId === b.endpointId && a.schema === b.schema && a.table === b.table,
  });
  useMouseNavButtons(panelRef, history);

  /** Open a table and record it in history. */
  const openTable = useCallback(
    (next: Selection) => {
      history.visit(next);
      void showTable(next);
    },
    [history, showTable],
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
      await showTable(sel, page);
    } catch (e) {
      setError(String(e));
    }
  };

  /** Delete the selected rows, after confirming — this cannot be undone. */
  const deleteRows = async (rowIndices: number[]) => {
    if (!sel || !rows || rowIndices.length === 0) return;
    const keys = rowIndices.map(rowKey);
    if (keys.some((k) => k === null)) {
      setError("This table has no primary key, so rows can't be deleted individually.");
      return;
    }
    const ok = await dialog.confirm({
      title: `Delete ${rowIndices.length} row${rowIndices.length === 1 ? "" : "s"}?`,
      message: `This permanently deletes ${rowIndices.length} row${
        rowIndices.length === 1 ? "" : "s"
      } from ${sel.schema}.${sel.table}. It can't be undone.`,
      danger: true,
      confirmText: "Delete",
    });
    if (!ok) return;
    setBusy(true);
    try {
      await api.dbDeleteRows(sel.sessionId, sel.schema, sel.table, keys as DbRowKey[]);
      await showTable(sel, page);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
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
      // Persist recent queries (phpMyAdmin-style history) per server.
      try {
        const key = `xconsole-sql-history:${data.vpsId}`;
        const next = [sql.trim(), ...sqlHistory.filter((q) => q !== sql.trim())].slice(
          0,
          40,
        );
        localStorage.setItem(key, JSON.stringify(next));
        setSqlHistory(next);
      } catch {
        /* ignore quota */
      }
    } catch (e) {
      setError(String(e));
      setSqlResult(null);
    } finally {
      setBusy(false);
    }
  };

  /** Export current grid (data tab or SQL result) as CSV via the browser download path. */
  const exportCsv = (set: DbResultSet | null, filename: string) => {
    if (!set || set.columns.length === 0) return;
    const esc = (v: string | null) => {
      const s = v ?? "";
      if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
      return s;
    };
    const lines = [
      set.columns.map(esc).join(","),
      ...set.rows.map((row) => row.map(esc).join(",")),
    ];
    const blob = new Blob([lines.join("\n")], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename.endsWith(".csv") ? filename : `${filename}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  /** Page through the table and export up to a safety cap (phpMyAdmin-style dump). */
  const exportAllCsv = async () => {
    if (!sel) return;
    const ok = await dialog.confirm({
      title: "Export full table CSV?",
      message: `Fetch ${sel.schema}.${sel.table} page-by-page (up to 50,000 rows) and download as CSV. Large tables may take a while.`,
      confirmText: "Export",
    });
    if (!ok) return;
    setBusy(true);
    setError(null);
    try {
      const esc = (v: string | null) => {
        const s = v ?? "";
        if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
        return s;
      };
      const MAX_ROWS = 50_000;
      const lines: string[] = [];
      let offset = 0;
      let cols: string[] | null = null;
      while (offset < MAX_ROWS) {
        const pageData = await api.dbSelectPage(
          sel.sessionId,
          sel.schema,
          sel.table,
          PAGE_SIZE,
          offset,
        );
        if (!cols) {
          cols = pageData.columns;
          lines.push(cols.map(esc).join(","));
        }
        for (const row of pageData.rows) lines.push(row.map(esc).join(","));
        if (pageData.rows.length < PAGE_SIZE) break;
        offset += PAGE_SIZE;
      }
      if (offset >= MAX_ROWS) {
        setError(`Export capped at ${MAX_ROWS} rows.`);
      }
      const blob = new Blob([lines.join("\n")], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${sel.schema}_${sel.table}_all.csv`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Export current page as INSERT statements (phpMyAdmin-style dump of visible rows). */
  const exportSqlInserts = (set: DbResultSet | null, tableLabel: string) => {
    if (!set || set.columns.length === 0 || set.rows.length === 0) return;
    const cols = set.columns.join(", ");
    const lines = set.rows.map((row) => {
      const vals = row
        .map((v) => {
          if (v === null) return "NULL";
          return `'${String(v).replace(/'/g, "''")}'`;
        })
        .join(", ");
      return `INSERT INTO ${tableLabel} (${cols}) VALUES (${vals});`;
    });
    const blob = new Blob([lines.join("\n") + "\n"], {
      type: "application/sql;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${tableLabel.replace(/[^a-zA-Z0-9_.-]+/g, "_")}_page.sql`;
    a.click();
    URL.revokeObjectURL(url);
  };

  /** Drop the current table after typing its name to confirm. */
  const dropTable = async () => {
    if (!sel) return;
    const typed = await dialog.prompt({
      title: `Drop table ${sel.schema}.${sel.table}?`,
      label: `Type the table name "${sel.table}" to confirm permanent DROP`,
      defaultValue: "",
      confirmText: "Drop table",
    });
    if (typed === null) return;
    if (typed.trim() !== sel.table) {
      setError("Drop cancelled — table name did not match.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.dbRunSql(sel.sessionId, `DROP TABLE ${sel.schema}.${sel.table}`);
      const droppedSchema = sel.schema;
      const droppedSession = sel.sessionId;
      setSel(null);
      setRows(null);
      setColumns([]);
      // Clear cached tables so the tree reloads without the dropped table.
      setInstances((prev) =>
        prev.map((inst) =>
          inst.sessionId === droppedSession ? { ...inst, tables: {} } : inst,
        ),
      );
      try {
        const tables = await api.dbListTables(droppedSession, droppedSchema);
        setInstances((prev) =>
          prev.map((inst) =>
            inst.sessionId === droppedSession
              ? { ...inst, tables: { ...inst.tables, [droppedSchema]: tables } }
              : inst,
          ),
        );
      } catch {
        /* tree will reload when schema is re-opened */
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Empty the current table (TRUNCATE / DELETE) after a strong confirm. */
  const truncateTable = async () => {
    if (!sel) return;
    const ok = await dialog.confirm({
      title: `Truncate ${sel.schema}.${sel.table}?`,
      message: `This deletes ALL rows in ${sel.schema}.${sel.table}. It cannot be undone from this app.`,
      danger: true,
      confirmText: "Truncate",
    });
    if (!ok) return;
    setBusy(true);
    setError(null);
    try {
      // Prefer TRUNCATE; fall back to DELETE for engines that refuse TRUNCATE.
      try {
        await api.dbRunSql(
          sel.sessionId,
          `TRUNCATE TABLE ${sel.schema}.${sel.table}`,
        );
      } catch {
        await api.dbRunSql(sel.sessionId, `DELETE FROM ${sel.schema}.${sel.table}`);
      }
      await showTable(sel, 0);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Import a .sql file from disk and run it against the current connection.
   *  Supports larger dumps (up to 64 MB) and naive multi-statement splitting so
   *  phpMyAdmin-style exports with many statements still land. */
  const importSqlFile = async () => {
    if (!sel?.sessionId) {
      setError("Connect to a database first.");
      return;
    }
    try {
      const picked = await api.pickFile("Import SQL file");
      if (!picked) return;
      setBusy(true);
      setError(null);
      const text = await api.localFsReadText(picked, 64 * 1024 * 1024);
      if (!text.trim()) {
        setError("SQL file is empty.");
        return;
      }
      // Strip SQL comments and split on semicolons at end-of-line (common dump style).
      // Falls back to whole-file exec if only one chunk.
      const statements = splitSqlStatements(text);
      let last: DbResultSet | null = null;
      let ok = 0;
      const errors: string[] = [];
      for (const stmt of statements) {
        try {
          last = await api.dbRunSql(sel.sessionId, stmt);
          ok += 1;
        } catch (e) {
          errors.push(String(e));
          // Keep going so a single bad statement does not abort a 10k-line dump mid-way.
          if (errors.length >= 20) {
            errors.push("…stopped after 20 statement errors");
            break;
          }
        }
      }
      if (last) setSqlResult(last);
      setTab("sql");
      setSql(
        text.length > 4000
          ? `${text.slice(0, 4000)}\n/* …truncated for editor · imported ${ok}/${statements.length} statements */`
          : text,
      );
      if (errors.length > 0) {
        setError(
          `Imported ${ok}/${statements.length} statements with ${errors.length} error(s): ${errors[0]}`,
        );
      } else if (statements.length > 1) {
        setError(null);
      }
      if (sel) await showTable(sel, page).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Insert a row by prompting for each column (phpMyAdmin-style quick insert). */
  const insertRow = async () => {
    if (!sel || columns.length === 0) return;
    const values: string[] = [];
    for (const col of columns) {
      const v = await dialog.prompt({
        title: `Insert · ${col.name}`,
        label: `${col.name} (${col.data_type})${col.nullable ? " — empty = NULL" : ""}`,
        defaultValue: col.default ?? "",
        confirmText: col === columns[columns.length - 1] ? "Insert" : "Next",
      });
      if (v === null) return; // cancelled
      values.push(v);
    }
    const vals = values
      .map((v, i) => {
        if (v === "" && columns[i].nullable) return "NULL";
        return `'${v.replace(/'/g, "''")}'`;
      })
      .join(", ");
    const engineSql = `INSERT INTO ${sel.schema}.${sel.table} (${columns
      .map((c) => c.name)
      .join(", ")}) VALUES (${vals})`;
    setBusy(true);
    setError(null);
    try {
      await api.dbRunSql(sel.sessionId, engineSql);
      await showTable(sel, page);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const connectedCount = instances.filter((i) => i.sessionId).length;

  return (
    <div
      ref={panelRef}
      className={`flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-violet-500" : "border-[var(--border)]"}`}
      onMouseDown={() => focus(id)}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
    >
      <NodeResizer
        minWidth={520}
        minHeight={280}
        // Always mounted, not just when selected: needing to click a node before you
        // could resize it was the whole reason edges were "hard to grab". The handles
        // stay invisible until hover — see .xc-resize-* in styles.css, which also gives
        // them a hit area far wider than the 1px line they draw.
        isVisible
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
          onClick={() => {
            // The one place a database connection is really finished with.
            for (const sid of sessionsRef.current) void api.dbDisconnect(sid).catch(() => {});
            sessionsRef.current.clear();
            removeNode(id);
          }}
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
              openTable({
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
              <button
                onClick={history.back}
                disabled={!history.canBack}
                className="rounded px-1 py-0.5 text-[11px] text-gray-400 hover:bg-[var(--border)] disabled:opacity-30"
                data-tooltip="Back (mouse button 4)"
              >
                ‹
              </button>
              <button
                onClick={history.forward}
                disabled={!history.canForward}
                className="mr-1 rounded px-1 py-0.5 text-[11px] text-gray-400 hover:bg-[var(--border)] disabled:opacity-30"
                data-tooltip="Forward (mouse button 5)"
              >
                ›
              </button>
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
                    type="button"
                    disabled={!rows || columns.length === 0 || busy}
                    onClick={() => void insertRow()}
                    className="rounded px-1.5 py-0.5 text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] disabled:opacity-30"
                    data-tooltip="Insert a new row"
                  >
                    Insert
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void truncateTable()}
                    className="rounded px-1.5 py-0.5 text-red-400/80 hover:bg-red-950/40 hover:text-red-300 disabled:opacity-30"
                    data-tooltip="Delete all rows in this table"
                  >
                    Truncate
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void dropTable()}
                    className="rounded px-1.5 py-0.5 text-red-500/90 hover:bg-red-950/50 hover:text-red-200 disabled:opacity-30"
                    data-tooltip="DROP TABLE permanently (type name to confirm)"
                  >
                    Drop
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void importSqlFile()}
                    className="rounded px-1.5 py-0.5 text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] disabled:opacity-30"
                    data-tooltip="Import a .sql file (up to 64 MB, multi-statement)"
                  >
                    Import
                  </button>
                  <button
                    type="button"
                    disabled={!rows}
                    onClick={() =>
                      exportCsv(rows, `${sel.schema}_${sel.table}_p${page + 1}.csv`)
                    }
                    className="rounded px-1.5 py-0.5 text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] disabled:opacity-30"
                    data-tooltip="Export this page as CSV"
                  >
                    CSV
                  </button>
                  <button
                    type="button"
                    disabled={busy || !sel}
                    onClick={() => void exportAllCsv()}
                    className="rounded px-1.5 py-0.5 text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] disabled:opacity-30"
                    data-tooltip="Export entire table as CSV (paged, max 50k rows)"
                  >
                    CSV all
                  </button>
                  <button
                    type="button"
                    disabled={!rows || (rows?.rows.length ?? 0) === 0}
                    onClick={() =>
                      exportSqlInserts(rows, `${sel.schema}.${sel.table}`)
                    }
                    className="rounded px-1.5 py-0.5 text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] disabled:opacity-30"
                    data-tooltip="Export this page as INSERT SQL"
                  >
                    SQL
                  </button>
                  <button
                    disabled={page === 0}
                    onClick={() => void showTable(sel, page - 1)}
                    className="rounded px-1 hover:bg-[var(--border)] disabled:opacity-30"
                  >
                    ‹
                  </button>
                  <span className="tabular-nums">
                    {page * PAGE_SIZE + 1}–{page * PAGE_SIZE + (rows?.rows.length ?? 0)}
                    {tableRowCount != null ? (
                      <span className="text-gray-600"> / {tableRowCount.toLocaleString()}</span>
                    ) : null}
                  </span>
                  <button
                    disabled={(rows?.rows.length ?? 0) < PAGE_SIZE}
                    onClick={() => void showTable(sel, page + 1)}
                    className="rounded px-1 hover:bg-[var(--border)] disabled:opacity-30"
                  >
                    ›
                  </button>
                </div>
              ) : null}
              {tab === "sql" ? (
                <div className="ml-auto flex items-center gap-1">
                  <button
                    type="button"
                    disabled={busy || !sel?.sessionId}
                    onClick={() => void importSqlFile()}
                    className="rounded px-1.5 py-0.5 text-[10px] text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] disabled:opacity-30"
                    data-tooltip="Import a .sql file (max 8 MB)"
                  >
                    Import
                  </button>
                  {sqlResult ? (
                    <button
                      type="button"
                      onClick={() => exportCsv(sqlResult, "query_result.csv")}
                      className="rounded px-1.5 py-0.5 text-[10px] text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
                      data-tooltip="Export query result as CSV"
                    >
                      CSV
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div className="min-h-0 flex-1 overflow-hidden">
              {tab === "data" ? (
                rows ? (
                  <Grid
                    set={rows}
                    columns={columns}
                    onEdit={(r, c, v) => void editCell(r, c, v)}
                    onDeleteRows={(idx) => void deleteRows(idx)}
                  />
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
                  <div className="flex h-full min-h-0 flex-col">
                    <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border)] px-2 py-1">
                      <button
                        type="button"
                        className="rounded px-1.5 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"
                        data-tooltip="Copy column definitions as SQL fragment"
                        onClick={() => {
                          const lines = columns.map((c) => {
                            const nullish = c.nullable ? "NULL" : "NOT NULL";
                            const def =
                              c.default != null && c.default !== ""
                                ? ` DEFAULT ${c.default}`
                                : "";
                            const key = c.primary ? " PRIMARY KEY" : "";
                            const extra = c.extra ? ` ${c.extra}` : "";
                            return `  ${c.name} ${c.data_type} ${nullish}${def}${key}${extra}`;
                          });
                          const body = lines.join(",\n");
                          const sqlFrag = sel
                            ? `CREATE TABLE ${sel.schema}.${sel.table} (\n${body}\n);`
                            : body;
                          void navigator.clipboard.writeText(sqlFrag);
                        }}
                      >
                        Copy CREATE
                      </button>
                      <button
                        type="button"
                        className="rounded px-1.5 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"
                        data-tooltip="Copy column names as comma-separated list"
                        onClick={() => {
                          void navigator.clipboard.writeText(
                            columns.map((c) => c.name).join(", "),
                          );
                        }}
                      >
                        Copy names
                      </button>
                      <span className="ml-auto text-[10px] text-gray-600">
                        {columns.length} column{columns.length === 1 ? "" : "s"}
                      </span>
                    </div>
                    <div className="min-h-0 flex-1">
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
                    </div>
                  </div>
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
                    <button
                      type="button"
                      onClick={toggleFavorite}
                      disabled={!sql.trim()}
                      className={`rounded px-1.5 py-0.5 text-[11px] disabled:opacity-30 ${
                        sqlFavorites.includes(sql.trim())
                          ? "text-amber-300"
                          : "text-gray-500 hover:text-gray-300"
                      }`}
                      data-tooltip={
                        sqlFavorites.includes(sql.trim())
                          ? "Remove from favorites"
                          : "Save query to favorites"
                      }
                    >
                      ★
                    </button>
                    {sqlFavorites.length > 0 ? (
                      <select
                        className="max-w-[200px] rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[10px] text-[var(--text-dim)]"
                        defaultValue=""
                        onChange={(e) => {
                          const v = e.target.value;
                          e.target.value = "";
                          if (!v) return;
                          // "run:" prefix = load into editor and execute immediately.
                          if (v.startsWith("run:")) {
                            const q = v.slice(4);
                            setSql(q);
                            void (async () => {
                              if (!sel?.sessionId) return;
                              setBusy(true);
                              setError(null);
                              try {
                                const result = await api.dbRunSql(sel.sessionId, q);
                                setSqlResult(result);
                              } catch (err) {
                                setError(String(err));
                              } finally {
                                setBusy(false);
                              }
                            })();
                          } else {
                            setSql(v);
                          }
                        }}
                        data-tooltip="Favorite queries — pick to load, or Run ★ to execute"
                      >
                        <option value="" disabled>
                          ★ Favorites ({sqlFavorites.length})
                        </option>
                        {sqlFavorites.map((q, i) => (
                          <option key={`f-${i}`} value={q}>
                            {q.length > 70 ? `${q.slice(0, 70)}…` : q}
                          </option>
                        ))}
                        <option disabled>────────</option>
                        {sqlFavorites.map((q, i) => (
                          <option key={`fr-${i}`} value={`run:${q}`}>
                            ▶ Run · {q.length > 50 ? `${q.slice(0, 50)}…` : q}
                          </option>
                        ))}
                      </select>
                    ) : null}
                    {sqlHistory.length > 0 ? (
                      <select
                        className="max-w-[200px] rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[10px] text-[var(--text-dim)]"
                        defaultValue=""
                        onChange={(e) => {
                          if (e.target.value) setSql(e.target.value);
                          e.target.value = "";
                        }}
                        data-tooltip="Query history"
                      >
                        <option value="" disabled>
                          History…
                        </option>
                        {sqlHistory.map((q, i) => (
                          <option key={i} value={q}>
                            {q.length > 80 ? `${q.slice(0, 80)}…` : q}
                          </option>
                        ))}
                      </select>
                    ) : null}
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
