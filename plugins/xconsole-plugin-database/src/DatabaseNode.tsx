import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { NodeResizer, useStore, type NodeProps } from "@xyflow/react";
import { api, type DbColumn, type DbResultSet, type DbRowKey } from "../../../src/lib/tauri";
import { useCanvasStore, type DbNode as DbNodeType } from "../../../src/stores/canvasStore";
import { useMouseNavButtons, useNavHistory } from "../../../src/hooks/useNavHistory";
import { dialog } from "../../../src/stores/dialogStore";
import { CodeEditArea } from "../../../src/components/CodeEditArea";
import { DatabaseTree, newInstance, type DbInstance } from "./DatabaseTree";
import {
  DatabaseIcon,
  SettingsIcon,
  PanelLeftIcon,
  PanelLeftCloseIcon,
  PanelRightIcon,
  PanelTopIcon,
  PanelBottomIcon,
  PlusIcon,
  TrashIcon,
  EraserIcon,
  DownloadIcon,
  UploadIcon,
  PlayIcon,
  StarFilledIcon,
  StarOutlineIcon,
  CopyIcon,
  CloseIcon,
  DuplicateIcon,
  SortAscIcon,
  SortDescIcon,
  TableIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  RefreshIcon,
  SlidersIcon,
  SearchIcon,
  FilterIcon,
} from "../../../src/components/icons";
import { InsertRowModal } from "./InsertRowModal";
import { CreateTableModal } from "./CreateTableModal";
import { AddColumnModal } from "./AddColumnModal";
import { RedisKeyModal } from "./RedisKeyModal";
import {
  DbSettingsModal,
  loadDbSettings,
  saveDbSettings,
  type DbSettings,
} from "./DbSettingsModal";
import { RowInspectorModal } from "./RowInspectorModal";

const PAGE_SIZE_OPTIONS = [25, 50, 100, 200, 500, 1000] as const;

/**
 * Split a SQL dump into statements for sequential import.
 * Handles `--` line comments and `/* GÇª *GÇï/` block comments.
 */
function splitSqlStatements(script: string): string[] {
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
  settings,
  onEdit,
  onDeleteRows,
  onSqlTemplate,
  onInspectRow,
  tableLabel,
  totalRows,
  activeFilter,
  onClearFilter,
}: {
  set: DbResultSet;
  columns?: DbColumn[];
  settings?: DbSettings;
  onEdit?: (rowIndex: number, column: string, next: string | null) => void;
  /** Delete the given row indices. Absent for result sets that aren't a real table. */
  onDeleteRows?: (rowIndices: number[]) => void;
  /** Open selected rows as SQL in the editor (INSERT template). */
  onSqlTemplate?: (sql: string) => void;
  onInspectRow?: (rowIndex: number) => void;
  tableLabel?: string;
  totalRows?: number | null;
  activeFilter?: string | null;
  onClearFilter?: () => void;
}) {
  const [editing, setEditing] = useState<{ row: number; col: number } | null>(null);
  const [draft, setDraft] = useState("");
  const [selected, setSelected] = useState<Set<number>>(() => new Set());
  const [sortCol, setSortCol] = useState<number | null>(null);
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc");
  const [filterText, setFilterText] = useState("");
  const [showFilterBar, setShowFilterBar] = useState(false);
  const lastClicked = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const pkColumns = (columns ?? []).filter((c) => c.primary);
  const hasKey = pkColumns.length > 0;
  const editable = Boolean(onEdit) && hasKey;
  const selectable = true;
  const canDelete = Boolean(onDeleteRows) && hasKey;

  const density = settings?.density ?? "compact";
  const fontSize = settings?.fontSize ?? "xs";
  const nullLabel = settings?.nullLabel ?? "NULL";
  const showRowNumbers = settings?.showRowNumbers ?? true;

  const cellPadding = density === "comfortable" ? "py-1.5 px-2.5" : "py-0.5 px-2";
  const textSize =
    fontSize === "sm" ? "text-[12px]" : fontSize === "base" ? "text-[13px]" : "text-[11px]";

  // A new result set invalidates the old indices
  useEffect(() => {
    setSelected(new Set());
    lastClicked.current = null;
    setSortCol(null);
    setFilterText("");
  }, [set]);

  /** Filter and sort rows locally on this page */
  const processedRows = useMemo(() => {
    let list = set.rows.map((row, originalIndex) => ({ row, originalIndex }));

    if (filterText.trim()) {
      const q = filterText.trim().toLowerCase();
      list = list.filter((item) =>
        item.row.some((cell) => cell != null && cell.toLowerCase().includes(q)),
      );
    }

    if (sortCol !== null && sortCol < set.columns.length) {
      list.sort((a, b) => {
        const va = a.row[sortCol];
        const vb = b.row[sortCol];
        if (va === null && vb === null) return 0;
        if (va === null) return 1;
        if (vb === null) return -1;
        const na = Number(va);
        const nb = Number(vb);
        if (!isNaN(na) && !isNaN(nb)) {
          return sortDir === "asc" ? na - nb : nb - na;
        }
        return sortDir === "asc"
          ? va.localeCompare(vb, undefined, { numeric: true })
          : vb.localeCompare(va, undefined, { numeric: true });
      });
    }

    return list;
  }, [set.rows, set.columns.length, filterText, sortCol, sortDir]);

  /** Click, ctrl-click to toggle, shift-click for a range */
  const toggleRow = (originalIndex: number, e: React.MouseEvent) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (e.shiftKey && lastClicked.current !== null) {
        const [from, to] = [lastClicked.current, originalIndex].sort((a, b) => a - b);
        for (let i = from; i <= to; i += 1) next.add(i);
      } else if (next.has(originalIndex)) {
        next.delete(originalIndex);
      } else {
        next.add(originalIndex);
      }
      return next;
    });
    lastClicked.current = originalIndex;
  };

  const allSelected =
    processedRows.length > 0 &&
    processedRows.every((item) => selected.has(item.originalIndex));

  const toggleSort = (colIndex: number) => {
    if (sortCol === colIndex) {
      if (sortDir === "asc") {
        setSortDir("desc");
      } else {
        setSortCol(null);
        setSortDir("asc");
      }
    } else {
      setSortCol(colIndex);
      setSortDir("asc");
    }
  };

  // Ctrl+wheel scrolls sideways
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
    <div className="flex h-full min-h-0 flex-col select-none">
      {/* Quick in-grid filter bar toggle */}
      {showFilterBar && (
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border)] bg-[var(--surface-2)] px-2 py-1">
          <SearchIcon size={12} className="text-gray-500" />
          <input
            type="text"
            autoFocus
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setFilterText("");
                setShowFilterBar(false);
              }
            }}
            placeholder="Quick search loaded rows on this page (Esc to close)GÇª"
            className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-200 outline-none focus:border-violet-500"
          />
          {filterText && (
            <span className="font-mono text-[10px] text-violet-300 shrink-0">
              {processedRows.length} / {set.rows.length} rows
            </span>
          )}
          <button
            type="button"
            onClick={() => {
              setFilterText("");
              setShowFilterBar(false);
            }}
            className="rounded p-0.5 text-gray-400 hover:text-white"
          >
            <CloseIcon size={11} />
          </button>
        </div>
      )}

      {/* Selected rows toolbar */}
      {selected.size > 0 ? (
        <div className="flex shrink-0 items-center gap-1.5 border-b border-[var(--border)] bg-violet-950/40 px-2 py-1 text-[11px] overflow-x-auto">
          <span className="font-medium text-violet-200 shrink-0">
            {selected.size} row{selected.size === 1 ? "" : "s"} selected
          </span>
          <button
            onClick={() => setSelected(new Set())}
            className="rounded px-1 text-gray-400 hover:text-gray-200"
          >
            Clear
          </button>
          <div className="mx-1 h-3 w-px bg-[var(--border)]" />
          <button
            type="button"
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--border)]"
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
            <CopyIcon size={11} />
            <span>JSON</span>
          </button>
          <button
            type="button"
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--border)]"
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
            <DownloadIcon size={11} />
            <span>CSV</span>
          </button>
          <button
            type="button"
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--border)]"
            data-tooltip="Copy selected rows as Markdown table"
            onClick={() => {
              const idxs = [...selected].sort((a, b) => a - b);
              const cell = (v: string | null) =>
                String(v ?? "NULL").replace(/\|/g, "\\|").replace(/\n/g, " ");
              const header = `| ${set.columns.map(cell).join(" | ")} |`;
              const sep = `| ${set.columns.map(() => "---").join(" | ")} |`;
              const body = idxs.map((i) => `| ${set.rows[i].map(cell).join(" | ")} |`);
              void navigator.clipboard.writeText([header, sep, ...body].join("\n"));
            }}
          >
            <CopyIcon size={11} />
            <span>MD</span>
          </button>
          <button
            type="button"
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--border)]"
            data-tooltip="Copy selected as INSERT statements"
            onClick={() => {
              const idxs = [...selected].sort((a, b) => a - b);
              const cols = set.columns.join(", ");
              const tbl = tableLabel || "/*table*/";
              const lines = idxs.map((i) => {
                const vals = set.rows[i]
                  .map((v) => {
                    if (v === null) return "NULL";
                    return `'${String(v).replace(/'/g, "''")}'`;
                  })
                  .join(", ");
                return `INSERT INTO ${tbl} (${cols}) VALUES (${vals});`;
              });
              void navigator.clipboard.writeText(lines.join("\n"));
            }}
          >
            <CopyIcon size={11} />
            <span>INSERT</span>
          </button>
          {onSqlTemplate ? (
            <button
              type="button"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 text-violet-300 hover:bg-[var(--border)]"
              data-tooltip="Load selected as INSERT in the SQL tab"
              onClick={() => {
                const idxs = [...selected].sort((a, b) => a - b);
                const cols = set.columns.join(", ");
                const tbl = tableLabel || "/*table*/";
                const lines = idxs.map((i) => {
                  const vals = set.rows[i]
                    .map((v) => {
                      if (v === null) return "NULL";
                      return `'${String(v).replace(/'/g, "''")}'`;
                    })
                    .join(", ");
                  return `INSERT INTO ${tbl} (${cols}) VALUES (${vals});`;
                });
                onSqlTemplate(lines.join("\n"));
              }}
            >
              <span>GåÆ SQL</span>
            </button>
          ) : null}
          {canDelete ? (
            <button
              onClick={() => onDeleteRows?.([...selected].sort((a, b) => a - b))}
              className="ml-auto flex items-center gap-1 rounded bg-red-700/90 px-2 py-0.5 text-white hover:bg-red-600 transition-colors"
              data-tooltip="Delete selected rows"
            >
              <TrashIcon size={11} />
              <span>Delete</span>
            </button>
          ) : null}
        </div>
      ) : null}

      {/* The scroll container */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto">
        <table className={`w-max min-w-full border-collapse text-left font-mono ${textSize}`}>
          <thead className="sticky top-0 z-10 bg-[var(--surface)] shadow-xs">
            <tr>
              {selectable ? (
                <th className="sticky left-0 z-20 w-6 border-b border-r border-[var(--border)] bg-[var(--surface)] px-1 py-1 text-center">
                  <input
                    type="checkbox"
                    checked={allSelected}
                    onChange={() =>
                      setSelected(
                        allSelected
                          ? new Set()
                          : new Set(processedRows.map((item) => item.originalIndex)),
                      )
                    }
                    data-tooltip="Select all on this page"
                    className="accent-violet-600 rounded"
                  />
                </th>
              ) : null}

              {showRowNumbers && (
                <th className="sticky left-6 z-20 w-8 border-b border-r border-[var(--border)] bg-[var(--surface)] px-1.5 py-1 text-right text-[10px] text-gray-500 font-normal">
                  <button
                    type="button"
                    onClick={() => setShowFilterBar((v) => !v)}
                    className="hover:text-violet-300"
                    data-tooltip="Quick search rows on this page (Ctrl+F)"
                  >
                    #
                  </button>
                </th>
              )}

              {set.columns.map((c, colIdx) => {
                const meta = columns?.find((m) => m.name === c);
                const isSorted = sortCol === colIdx;
                return (
                  <th
                    key={c}
                    onClick={() => toggleSort(colIdx)}
                    className="cursor-pointer whitespace-nowrap border-b border-r border-[var(--border)] px-2 py-1 font-medium text-gray-300 hover:bg-[var(--surface-hover)] transition-colors last:border-r-0"
                    title={
                      meta
                        ? `${meta.data_type}${meta.primary ? " -+ Primary key" : ""}`
                        : c
                    }
                  >
                    <div className="flex items-center gap-1.5">
                      {meta?.primary ? (
                        <span className="text-amber-400 text-[10px]">=ƒöæ</span>
                      ) : null}
                      <span>{c}</span>
                      {isSorted ? (
                        sortDir === "asc" ? (
                          <SortAscIcon size={11} className="text-violet-400" />
                        ) : (
                          <SortDescIcon size={11} className="text-violet-400" />
                        )
                      ) : (
                        <span className="opacity-0 hover:opacity-50 text-[9px] text-gray-500">
                          Gçà
                        </span>
                      )}
                    </div>
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {processedRows.map(({ row, originalIndex }) => {
              const isSelected = selected.has(originalIndex);
              return (
                <tr
                  key={originalIndex}
                  className={`group transition-colors ${
                    isSelected ? "bg-violet-600/25" : "hover:bg-[var(--border)]/40"
                  }`}
                >
                  {selectable ? (
                    <td
                      className={`sticky left-0 z-10 border-b border-r border-[var(--border)] px-1 text-center ${
                        isSelected ? "bg-violet-900/60" : "bg-[var(--bg)]"
                      }`}
                      onClick={(e) => toggleRow(originalIndex, e)}
                    >
                      <input
                        type="checkbox"
                        checked={isSelected}
                        readOnly
                        tabIndex={-1}
                        className="accent-violet-600 rounded pointer-events-none"
                      />
                    </td>
                  ) : null}

                  {showRowNumbers && (
                    <td
                      className={`sticky left-6 z-10 border-b border-r border-[var(--border)] px-1.5 text-right font-mono text-[10px] text-gray-600 cursor-pointer hover:text-violet-300 ${
                        isSelected ? "bg-violet-900/60" : "bg-[var(--bg)]"
                      }`}
                      onClick={() => onInspectRow?.(originalIndex)}
                      data-tooltip="Click or double-click to inspect & edit row"
                    >
                      {originalIndex + 1}
                    </td>
                  )}

                  {row.map((cell, ci) => {
                    const isEditing = editing?.row === originalIndex && editing?.col === ci;
                    return (
                      <td
                        key={ci}
                        className={`max-w-[340px] truncate border-b border-r border-[var(--border)] ${cellPadding} text-gray-300 last:border-r-0 select-text`}
                        title={cell ?? "NULL"}
                        onDoubleClick={() => {
                          if (onInspectRow) {
                            onInspectRow(originalIndex);
                            return;
                          }
                          if (!editable) return;
                          setEditing({ row: originalIndex, col: ci });
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
                                onEdit?.(originalIndex, set.columns[ci], draft);
                                setEditing(null);
                              }
                            }}
                            className="w-full rounded bg-[var(--bg)] border border-violet-500 px-1 py-0 text-[11px] text-gray-100 outline-none"
                          />
                        ) : cell === null ? (
                          <span className="italic text-gray-600 font-mono text-[10px]">
                            {nullLabel}
                          </span>
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

        {processedRows.length === 0 ? (
          <div className="p-6 text-center text-gray-500 text-[11px]">
            <p>{filterText ? "No rows match your local page search." : "No rows found."}</p>
            {activeFilter && (
              <button
                type="button"
                onClick={onClearFilter}
                className="mt-2 rounded bg-violet-600/30 px-2 py-1 text-violet-200 hover:bg-violet-600/50"
              >
                Clear Database Filter ({activeFilter})
              </button>
            )}
          </div>
        ) : null}
      </div>

      {/* Slim Status Bar at Bottom */}
      <div className="flex shrink-0 items-center justify-between border-t border-[var(--border)] bg-[var(--surface-2)] px-2 py-0.5 text-[10px] text-gray-500 font-mono select-none">
        <div className="flex items-center gap-2">
          {tableLabel && <span className="text-gray-400 font-medium">{tableLabel}</span>}
          {pkColumns.length > 0 ? (
            <span className="text-amber-400/80">
              PK: {pkColumns.map((c) => c.name).join(", ")}
            </span>
          ) : (
            <span className="text-gray-600">No PK</span>
          )}
          <span>{set.columns.length} columns</span>
          {activeFilter && (
            <span className="text-violet-300 bg-violet-950/60 px-1 rounded border border-violet-800/40">
              Filter: {activeFilter}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {totalRows != null && (
            <span>{totalRows.toLocaleString()} total rows</span>
          )}
          <span>{processedRows.length} on page</span>
        </div>
      </div>
    </div>
  );
}

/**
 * A comprehensive, responsive database browser for MySQL / MariaDB, PostgreSQL, Redis / Valkey.
 */
export const DatabaseNode = memo(function DatabaseNode({
  id,
  data,
  selected,
}: NodeProps<DbNodeType>) {
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);

  const [instances, setInstances] = useState<DbInstance[]>([]);
  const [scanning, setScanning] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [sidebarOpen, setSidebarOpen] = useState(() => {
    return localStorage.getItem("xconsole-db-sidebar-open") !== "false";
  });
  const setSidebarOpenPersist = (val: boolean) => {
    setSidebarOpen(val);
    try {
      localStorage.setItem("xconsole-db-sidebar-open", String(val));
    } catch {
      /* ignore */
    }
  };

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<DbSettings>(loadDbSettings);
  const [inspectRowIndex, setInspectRowIndex] = useState<number | null>(null);

  const [sel, setSel] = useState<Selection | null>(null);
  const [columns, setColumns] = useState<DbColumn[]>([]);
  const [rows, setRows] = useState<DbResultSet | null>(null);
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState<number>(() => {
    return settings.pageSize || 200;
  });
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [tab, setTab] = useState<Tab>(() => {
    const t = localStorage.getItem("xconsole-db-tab");
    return t === "data" || t === "structure" || t === "sql" ? t : "data";
  });
  const setTabPersist = (t: Tab) => {
    setTab(t);
    try {
      localStorage.setItem("xconsole-db-tab", t);
    } catch {
      /* ignore */
    }
  };

  // Advanced Table Filter & Search Builder state
  const [filterBarOpen, setFilterBarOpen] = useState(false);
  const [filterCol, setFilterCol] = useState("");
  const [filterOp, setFilterOp] = useState<string>("contains");
  const [filterVal, setFilterVal] = useState("");
  const [activeFilterWhere, setActiveFilterWhere] = useState<string | null>(null);

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

  // Modals for CRUD operations
  const [insertOpen, setInsertOpen] = useState(false);
  const [createTableOpen, setCreateTableOpen] = useState(false);
  const [addColumnOpen, setAddColumnOpen] = useState(false);
  const [redisKeyInspect, setRedisKeyInspect] = useState<{
    key: string;
    type?: string;
    ttl?: string;
  } | null>(null);

  const sessionsRef = useRef<Set<string>>(new Set());
  const panelRef = useRef<HTMLDivElement | null>(null);

  // Table Tabs filtering
  const [tableTabsFilter, setTableTabsFilter] = useState("");

  const updateSetting = useCallback(
    <K extends keyof DbSettings>(key: K, value: DbSettings[K]) => {
      setSettings((prev) => {
        const next = { ...prev, [key]: value };
        saveDbSettings(next);
        return next;
      });
    },
    [],
  );

  // Layout position cycling (Left -> Top -> Right -> Bottom)
  const cycleLayout = () => {
    const order: DbSettings["sidebarPosition"][] = ["left", "top", "right", "bottom"];
    const curIdx = order.indexOf(settings.sidebarPosition);
    const nextPos = order[(curIdx + 1) % order.length];
    updateSetting("sidebarPosition", nextPos);
  };

  // Draggable Resizing logic
  const isDraggingRef = useRef(false);
  const startPosRef = useRef(0);
  const startDimRef = useRef(0);

  const handleResizeStart = (e: React.MouseEvent, type: "width" | "height") => {
    e.preventDefault();
    e.stopPropagation();
    isDraggingRef.current = true;
    startPosRef.current = type === "width" ? e.clientX : e.clientY;
    startDimRef.current =
      type === "width" ? settings.sidebarWidth : settings.sidebarHeight;

    const onMouseMove = (moveEvent: MouseEvent) => {
      if (!isDraggingRef.current) return;
      if (type === "width") {
        const delta =
          settings.sidebarPosition === "right"
            ? startPosRef.current - moveEvent.clientX
            : moveEvent.clientX - startPosRef.current;
        const newWidth = Math.max(140, Math.min(600, startDimRef.current + delta));
        updateSetting("sidebarWidth", newWidth);
      } else {
        const delta =
          settings.sidebarPosition === "bottom"
            ? startPosRef.current - moveEvent.clientY
            : moveEvent.clientY - startPosRef.current;
        const newHeight = Math.max(90, Math.min(450, startDimRef.current + delta));
        updateSetting("sidebarHeight", newHeight);
      }
    };

    const onMouseUp = () => {
      isDraggingRef.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  };

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const [found, saved] = await Promise.all([
        api.dbDiscover(data.vpsId),
        api.dbListConnections(data.vpsId).catch(() => []),
      ]);
      const savedByEndpoint = new Map(saved.map((s) => [s.endpoint_id, s]));
      setInstances((prev) => {
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

  const refreshSaved = useCallback(async () => {
    try {
      const saved = await api.dbListConnections(data.vpsId);
      const byEndpoint = new Map(saved.map((s) => [s.endpoint_id, s]));
      setInstances((prev) =>
        prev.map((i) => ({ ...i, saved: byEndpoint.get(i.endpoint.id) })),
      );
    } catch {
      /* ignore */
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

  const patch = useCallback((endpointId: string, p: Partial<DbInstance>) => {
    if (p.sessionId) sessionsRef.current.add(p.sessionId);
    setInstances((prev) =>
      prev.map((i) => (i.endpoint.id === endpointId ? { ...i, ...p } : i)),
    );
  }, []);

  const [tableRowCount, setTableRowCount] = useState<number | null>(null);

  /** Load a table with optional WHERE filter */
  const showTable = useCallback(
    async (next: Selection, atPage = 0, whereClause: string | null = activeFilterWhere) => {
      setSel(next);
      setPage(atPage);
      setBusy(true);
      setError(null);
      try {
        // Ensure session database context is set
        void api.dbUseDatabase(next.sessionId, next.schema).catch(() => {});

        const inst = instances.find((i) => i.endpoint.id === next.endpointId);
        const isPostgres = inst?.endpoint.engine === "postgres";

        const cols = await api.dbDescribeTable(next.sessionId, next.schema, next.table);
        setColumns(cols);

        let data: DbResultSet;
        if (whereClause && whereClause.trim()) {
          const offset = atPage * pageSize;
          const tableIdent = isPostgres ? `"${next.table}"` : `${next.schema}.${next.table}`;
          const selectSql = `SELECT * FROM ${tableIdent} WHERE ${whereClause} LIMIT ${pageSize} OFFSET ${offset};`;
          data = await api.dbRunSql(next.sessionId, selectSql);
        } else {
          data = await api.dbSelectPage(
            next.sessionId,
            next.schema,
            next.table,
            pageSize,
            atPage * pageSize,
          );
        }
        setRows(data);
        setTab("data");

        // Best-effort count query
        if (atPage === 0) {
          const tableIdent = isPostgres ? `"${next.table}"` : `${next.schema}.${next.table}`;
          const wherePart = whereClause && whereClause.trim() ? ` WHERE ${whereClause}` : "";
          const countSql = `SELECT COUNT(*) AS c FROM ${tableIdent}${wherePart}`;

          void api
            .dbRunSql(next.sessionId, countSql)
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
    [pageSize, instances, activeFilterWhere],
  );

  // Optional live refresh
  useEffect(() => {
    if (!autoRefresh || !sel || tab !== "data") return;
    const sessionId = sel.sessionId;
    const schema = sel.schema;
    const table = sel.table;
    const at = page;
    const size = pageSize;
    const intervalMs = (settings.refreshInterval || 5) * 1000;
    const t = window.setInterval(() => {
      void api
        .dbSelectPage(sessionId, schema, table, size, at * size)
        .then((data) => setRows(data))
        .catch(() => {});
    }, intervalMs);
    return () => window.clearInterval(t);
  }, [autoRefresh, sel, page, pageSize, tab, settings.refreshInterval]);

  const history = useNavHistory<Selection>({
    current: sel,
    go: useCallback((entry: Selection) => void showTable(entry), [showTable]),
    isSame: (a, b) =>
      a.endpointId === b.endpointId && a.schema === b.schema && a.table === b.table,
  });
  useMouseNavButtons(panelRef, history);

  /** Open a table and record it in history */
  const openTable = useCallback(
    (next: Selection) => {
      setActiveFilterWhere(null);
      setFilterVal("");
      history.visit(next);
      void showTable(next, 0, null);
    },
    [history, showTable],
  );

  // Build WHERE condition from filter inputs
  const applyFilter = () => {
    if (!sel) return;
    const inst = instances.find((i) => i.endpoint.id === sel.endpointId);
    const isPostgres = inst?.endpoint.engine === "postgres";
    const val = filterVal.trim();

    let where: string | null = null;
    const quoteCol = (c: string) => (isPostgres ? `"${c}"` : `\`${c}\``);

    if (filterOp === "null") {
      where = filterCol ? `${quoteCol(filterCol)} IS NULL` : null;
    } else if (filterOp === "notnull") {
      where = filterCol ? `${quoteCol(filterCol)} IS NOT NULL` : null;
    } else if (val) {
      const escVal = val.replace(/'/g, "''");
      if (filterCol) {
        const colQ = isPostgres ? `CAST(${quoteCol(filterCol)} AS TEXT)` : quoteCol(filterCol);
        if (filterOp === "contains") where = `${colQ} LIKE '%${escVal}%'`;
        else if (filterOp === "equals") where = `${colQ} = '${escVal}'`;
        else if (filterOp === "starts") where = `${colQ} LIKE '${escVal}%'`;
        else if (filterOp === "ends") where = `${colQ} LIKE '%${escVal}'`;
        else if (filterOp === "neq") where = `${colQ} != '${escVal}'`;
        else if (filterOp === "gt") where = `${colQ} > '${escVal}'`;
        else if (filterOp === "lt") where = `${colQ} < '${escVal}'`;
        else if (filterOp === "gte") where = `${colQ} >= '${escVal}'`;
        else if (filterOp === "lte") where = `${colQ} <= '${escVal}'`;
      } else {
        // Search across all columns
        const parts = columns.map((c) => {
          const colQ = isPostgres ? `CAST(${quoteCol(c.name)} AS TEXT)` : quoteCol(c.name);
          return `${colQ} LIKE '%${escVal}%'`;
        });
        if (parts.length > 0) where = `(${parts.join(" OR ")})`;
      }
    }

    setActiveFilterWhere(where);
    void showTable(sel, 0, where);
  };

  const clearFilter = () => {
    setActiveFilterWhere(null);
    setFilterVal("");
    if (sel) void showTable(sel, 0, null);
  };

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

  const handleSaveRowInspector = async (
    rowIndex: number,
    updatedValues: Record<string, string | null>,
  ) => {
    const key = rowKey(rowIndex);
    if (!sel || !key || !rows) return;
    const originalRow = rows.rows[rowIndex];
    setBusy(true);
    setError(null);
    try {
      for (let ci = 0; ci < rows.columns.length; ci += 1) {
        const colName = rows.columns[ci];
        const oldVal = originalRow[ci];
        const newVal = updatedValues[colName];
        if (newVal !== oldVal) {
          await api.dbUpdateCell(
            sel.sessionId,
            sel.schema,
            sel.table,
            colName,
            newVal,
            key,
          );
        }
      }
      await showTable(sel, page);
    } catch (e) {
      setError(String(e));
      throw e;
    } finally {
      setBusy(false);
    }
  };

  const deleteRows = async (rowIndices: number[]) => {
    if (!sel || !rows || rowIndices.length === 0) return;
    const keys = rowIndices.map(rowKey);
    if (keys.some((k) => k === null)) {
      setError("This table has no primary key, so rows can't be deleted individually.");
      return;
    }
    if (settings.confirmDestructive) {
      const ok = await dialog.confirm({
        title: `Delete ${rowIndices.length} row${rowIndices.length === 1 ? "" : "s"}?`,
        message: `Permanently delete ${rowIndices.length} row${
          rowIndices.length === 1 ? "" : "s"
        } from ${sel.schema}.${sel.table}. This cannot be undone.`,
        danger: true,
        confirmText: "Delete",
      });
      if (!ok) return;
    }
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
      setError("Connect to a server and select a database first.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const statements = splitSqlStatements(sql);
      if (statements.length > 1) {
        let lastResult: DbResultSet | null = null;
        let totalAffected = 0;
        let executed = 0;
        for (const stmt of statements) {
          const res = await api.dbRunSql(sel.sessionId, stmt);
          lastResult = res;
          executed += 1;
          if (typeof res.affected === "number") totalAffected += res.affected;
        }
        if (lastResult) {
          if (!lastResult.message && executed > 1) {
            lastResult = {
              ...lastResult,
              message: `Executed ${executed} statements successfully. Total affected rows: ${totalAffected}`,
            };
          }
          setSqlResult(lastResult);
        }
      } else {
        setSqlResult(await api.dbRunSql(sel.sessionId, sql));
      }
      try {
        const key = `xconsole-sql-history:${data.vpsId}`;
        const next = [sql.trim(), ...sqlHistory.filter((q) => q !== sql.trim())].slice(
          0,
          40,
        );
        localStorage.setItem(key, JSON.stringify(next));
        setSqlHistory(next);
      } catch {
        /* ignore */
      }
    } catch (e) {
      setError(String(e));
      setSqlResult(null);
    } finally {
      setBusy(false);
    }
  };

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

  const exportAllCsv = async () => {
    if (!sel) return;
    const maxCap = settings.maxExportLimit || 50000;
    const ok = await dialog.confirm({
      title: "Export full table CSV?",
      message: `Fetch ${sel.schema}.${sel.table} page-by-page (up to ${maxCap.toLocaleString()} rows) and download as CSV.`,
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
      const lines: string[] = [];
      let offset = 0;
      let cols: string[] | null = null;
      while (offset < maxCap) {
        const pageData = await api.dbSelectPage(
          sel.sessionId,
          sel.schema,
          sel.table,
          pageSize,
          offset,
        );
        if (!cols) {
          cols = pageData.columns;
          lines.push(cols.map(esc).join(","));
        }
        for (const row of pageData.rows) lines.push(row.map(esc).join(","));
        if (pageData.rows.length < pageSize) break;
        offset += pageSize;
      }
      if (offset >= maxCap) {
        setError(`Export reached limit of ${maxCap.toLocaleString()} rows.`);
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

  const duplicateTable = async () => {
    if (!sel) return;
    const newName = await dialog.prompt({
      title: `Duplicate ${sel.table}`,
      label: "Enter new table name:",
      defaultValue: `${sel.table}_copy`,
      confirmText: "Duplicate",
    });
    if (!newName || !newName.trim()) return;
    const targetName = newName.trim();
    const inst = instances.find((i) => i.endpoint.id === sel.endpointId);
    const isPostgres = inst?.endpoint.engine === "postgres";

    setBusy(true);
    setError(null);
    try {
      if (isPostgres) {
        await api.dbRunSql(
          sel.sessionId,
          `CREATE TABLE "${targetName}" AS TABLE "${sel.table}";`,
        );
      } else {
        await api.dbRunSql(
          sel.sessionId,
          `CREATE TABLE ${sel.schema}.${targetName} LIKE ${sel.schema}.${sel.table};`,
        );
        await api.dbRunSql(
          sel.sessionId,
          `INSERT INTO ${sel.schema}.${targetName} SELECT * FROM ${sel.schema}.${sel.table};`,
        );
      }
      const updated = await api.dbListTables(sel.sessionId, sel.schema);
      patch(sel.endpointId, {
        tables: {
          ...(inst?.tables ?? {}),
          [sel.schema]: updated,
        },
      });
      openTable({
        endpointId: sel.endpointId,
        sessionId: sel.sessionId,
        schema: sel.schema,
        table: targetName,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const dropTable = async () => {
    if (!sel) return;
    const inst = instances.find((i) => i.endpoint.id === sel.endpointId);
    const isPostgres = inst?.endpoint.engine === "postgres";

    if (settings.confirmDestructive) {
      const typed = await dialog.prompt({
        title: `Drop table ${sel.schema}.${sel.table}?`,
        label: `Type table name "${sel.table}" to confirm permanent DROP:`,
        defaultValue: "",
        confirmText: "Drop table",
      });
      if (typed === null) return;
      if (typed.trim() !== sel.table) {
        setError("Drop cancelled GÇö table name did not match.");
        return;
      }
    }
    setBusy(true);
    setError(null);
    try {
      const dropSql = isPostgres
        ? `DROP TABLE "${sel.table}"`
        : `DROP TABLE ${sel.schema}.${sel.table}`;
      await api.dbRunSql(sel.sessionId, dropSql);
      const droppedSchema = sel.schema;
      const droppedSession = sel.sessionId;
      setSel(null);
      setRows(null);
      setColumns([]);
      try {
        const tables = await api.dbListTables(droppedSession, droppedSchema);
        patch(sel.endpointId, {
          tables: { ...(inst?.tables ?? {}), [droppedSchema]: tables },
        });
      } catch {
        /* ignore */
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const truncateTable = async () => {
    if (!sel) return;
    const inst = instances.find((i) => i.endpoint.id === sel.endpointId);
    const isPostgres = inst?.endpoint.engine === "postgres";

    if (settings.confirmDestructive) {
      const ok = await dialog.confirm({
        title: `Truncate ${sel.schema}.${sel.table}?`,
        message: `Delete ALL rows in ${sel.schema}.${sel.table}. This cannot be undone.`,
        danger: true,
        confirmText: "Truncate",
      });
      if (!ok) return;
    }
    setBusy(true);
    setError(null);
    try {
      const truncSql = isPostgres
        ? `TRUNCATE TABLE "${sel.table}"`
        : `TRUNCATE TABLE ${sel.schema}.${sel.table}`;
      try {
        await api.dbRunSql(sel.sessionId, truncSql);
      } catch {
        const delSql = isPostgres
          ? `DELETE FROM "${sel.table}"`
          : `DELETE FROM ${sel.schema}.${sel.table}`;
        await api.dbRunSql(sel.sessionId, delSql);
      }
      await showTable(sel, 0);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

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
          if (errors.length >= 20) {
            errors.push("GÇªstopped after 20 statement errors");
            break;
          }
        }
      }
      if (last) setSqlResult(last);
      setTab("sql");
      setSql(
        text.length > 4000
          ? `${text.slice(0, 4000)}\n/* GÇªimported ${ok}/${statements.length} statements */`
          : text,
      );
      if (errors.length > 0) {
        setError(
          `Imported ${ok}/${statements.length} statements with ${errors.length} error(s): ${errors[0]}`,
        );
      }
      if (sel) await showTable(sel, page).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDisconnect = async (inst: DbInstance) => {
    if (inst.sessionId) {
      try {
        await api.dbDisconnect(inst.sessionId);
        sessionsRef.current.delete(inst.sessionId);
        patch(inst.endpoint.id, {
          sessionId: null,
          schemas: [],
          tables: {},
          openSchemas: [],
        });
        if (sel?.endpointId === inst.endpoint.id) {
          setSel(null);
          setRows(null);
          setColumns([]);
        }
      } catch {
        /* ignore */
      }
    }
  };

  const connectedCount = instances.filter((i) => i.sessionId).length;

  // Active instance & schema tables for Table Tabs Bar
  const activeInstance = instances.find((i) => i.endpoint.id === sel?.endpointId);
  const activeSchemaTables = useMemo(() => {
    if (!sel || !activeInstance) return [];
    return activeInstance.tables[sel.schema] ?? [];
  }, [sel, activeInstance]);

  const showTableTabsBar =
    (settings.tableTabsMode === "always" && activeSchemaTables.length > 0) ||
    (settings.tableTabsMode === "when-hidden" &&
      !sidebarOpen &&
      activeSchemaTables.length > 0) ||
    (!sidebarOpen && activeSchemaTables.length > 0);

  const filteredSchemaTables = useMemo(() => {
    if (!tableTabsFilter.trim()) return activeSchemaTables;
    const q = tableTabsFilter.trim().toLowerCase();
    return activeSchemaTables.filter((t) => t.name.toLowerCase().includes(q));
  }, [activeSchemaTables, tableTabsFilter]);

  // Layout layout direction classes
  const pos = settings.sidebarPosition || "left";
  const isHorizontalLayout = pos === "top" || pos === "bottom";

  const renderLayoutIcon = () => {
    if (pos === "left") return <PanelLeftIcon size={13} />;
    if (pos === "right") return <PanelRightIcon size={13} />;
    if (pos === "top") return <PanelTopIcon size={13} />;
    return <PanelBottomIcon size={13} />;
  };

  const treeComponent = (
    <DatabaseTree
      instances={instances}
      vpsId={data.vpsId}
      scanning={scanning}
      selected={sel}
      position={pos}
      width={settings.sidebarWidth}
      height={settings.sidebarHeight}
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
      onForget={(fid) => void forgetSaved(fid)}
      onDisconnect={handleDisconnect}
    />
  );

  const splitterComponent = isHorizontalLayout ? (
    <div
      onMouseDown={(e) => handleResizeStart(e, "height")}
      className="group h-1 hover:h-1.5 bg-[var(--border)] hover:bg-violet-500 cursor-row-resize shrink-0 transition-colors z-20 flex items-center justify-center"
      data-tooltip="Drag to resize panel height"
    >
      <div className="w-8 h-0.5 rounded bg-gray-500 group-hover:bg-white transition-colors" />
    </div>
  ) : (
    <div
      onMouseDown={(e) => handleResizeStart(e, "width")}
      className="group w-1 hover:w-1.5 bg-[var(--border)] hover:bg-violet-500 cursor-col-resize shrink-0 transition-colors z-20 flex items-center justify-center"
      data-tooltip="Drag to resize panel width"
    >
      <div className="h-8 w-0.5 rounded bg-gray-500 group-hover:bg-white transition-colors" />
    </div>
  );

  return (
    <div
      ref={panelRef}
      className={`flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-violet-500" : "border-[var(--border)]"}`}
      onMouseDown={() => focus(id)}
      onKeyDown={(e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b") {
          e.preventDefault();
          setSidebarOpenPersist(!sidebarOpen);
        }
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f" && tab === "data") {
          e.preventDefault();
          setFilterBarOpen((v) => !v);
        }
      }}
      style={
        freeform
          ? undefined
          : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }
      }
    >
      <NodeResizer
        minWidth={480}
        minHeight={260}
        isVisible
        lineClassName="border-violet-500"
        handleClassName="h-2 w-2 rounded bg-violet-500"
      />

      {/* Top Title Bar */}
      <div
        className="flex shrink-0 cursor-move items-center gap-1.5 border-b border-[var(--border)] bg-[var(--surface)] px-2.5 py-1.5 select-none"
        onDoubleClick={() => focus(id)}
      >
        <DatabaseIcon size={13} className="shrink-0 text-violet-400" />
        <button
          type="button"
          className="truncate text-xs font-medium text-gray-200 hover:text-white"
          data-tooltip="Click to copy server name"
          onClick={(e) => {
            e.stopPropagation();
            void navigator.clipboard.writeText(String(data.name ?? ""));
          }}
        >
          {data.name}
        </button>
        <span className="shrink-0 text-[10px] text-gray-500 font-mono">
          {instances.length} inst{instances.length === 1 ? "" : "s"}
          {connectedCount > 0 ? ` -+ ${connectedCount} live` : ""}
        </span>

        {sel ? (
          <button
            type="button"
            className="truncate text-[10px] font-mono text-violet-300 hover:text-violet-100 max-w-[200px]"
            data-tooltip="Click to copy database.table"
            onClick={(e) => {
              e.stopPropagation();
              void navigator.clipboard.writeText(`${sel.schema}.${sel.table}`);
            }}
          >
            {sel.schema}.{sel.table}
          </button>
        ) : null}

        {/* Top-Right Tools */}
        <div className="ml-auto flex items-center gap-1 shrink-0">
          {/* Button 1: Toggle Hide / Show panel */}
          <button
            type="button"
            onClick={() => setSidebarOpenPersist(!sidebarOpen)}
            className={`rounded p-1 transition-colors ${
              sidebarOpen
                ? "text-gray-300 hover:bg-[var(--border)] hover:text-white"
                : "text-violet-400 bg-violet-950/60 border border-violet-800/60 hover:bg-violet-900/60"
            }`}
            data-tooltip={
              sidebarOpen
                ? `Hide ${pos} panel (Ctrl+B)`
                : `Show ${pos} panel (Ctrl+B)`
            }
          >
            {sidebarOpen ? <PanelLeftCloseIcon size={13} /> : <PanelLeftIcon size={13} />}
          </button>

          {/* Button 2: Rotate Panel Position (Left -> Top -> Right -> Bottom) */}
          <button
            type="button"
            onClick={cycleLayout}
            className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-violet-300 transition-colors"
            data-tooltip={`Rotate Position: ${pos.toUpperCase()} (Click to rotate Left GåÆ Top GåÆ Right GåÆ Bottom)`}
          >
            {renderLayoutIcon()}
          </button>

          {/* Button 3: Settings */}
          <button
            type="button"
            onClick={() => setSettingsOpen(true)}
            className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-violet-300 transition-colors"
            data-tooltip="Database manager settings"
          >
            <SettingsIcon size={13} />
          </button>

          {/* Button 4: Close */}
          <button
            type="button"
            className="rounded p-1 text-gray-500 hover:bg-[var(--border)] hover:text-white"
            onClick={() => {
              for (const sid of sessionsRef.current)
                void api.dbDisconnect(sid).catch(() => {});
              sessionsRef.current.clear();
              removeNode(id);
            }}
            data-tooltip="Close Database Manager"
          >
            <CloseIcon size={12} />
          </button>
        </div>
      </div>

      <div className="nodrag nowheel flex min-h-0 flex-1 flex-col">
        {error ? (
          <div className="flex shrink-0 items-start gap-2 border-b border-red-500/40 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
            <span className="min-w-0 flex-1 break-words">{error}</span>
            <button
              type="button"
              className="shrink-0 rounded px-1 text-red-400/80 hover:bg-red-900/30 hover:text-red-200"
              onClick={() => setError(null)}
              data-tooltip="Dismiss"
            >
              <CloseIcon size={10} />
            </button>
          </div>
        ) : null}

        {/* Responsive Flex Layout Container */}
        <div
          className={`flex min-h-0 flex-1 overflow-hidden ${
            pos === "top"
              ? "flex-col"
              : pos === "bottom"
                ? "flex-col-reverse"
                : pos === "right"
                  ? "flex-row-reverse"
                  : "flex-row"
          }`}
        >
          {/* Collapsible Tree Sidebar / Topbar */}
          {sidebarOpen && treeComponent}
          {sidebarOpen && splitterComponent}

          {/* Main Work Area */}
          <div className="flex min-w-0 min-h-0 flex-1 flex-col overflow-hidden">
            {/* Toolbar */}
            <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-[var(--border)] bg-[var(--surface-2)] px-2 py-1">
              <button
                onClick={history.back}
                disabled={!history.canBack}
                className="rounded p-1 text-gray-400 hover:bg-[var(--border)] disabled:opacity-30"
                data-tooltip="Back (mouse button 4)"
              >
                <ChevronLeftIcon size={12} />
              </button>
              <button
                onClick={history.forward}
                disabled={!history.canForward}
                className="mr-1 rounded p-1 text-gray-400 hover:bg-[var(--border)] disabled:opacity-30"
                data-tooltip="Forward (mouse button 5)"
              >
                <ChevronRightIcon size={12} />
              </button>

              {/* Tab Selector */}
              <div className="flex rounded border border-[var(--border)] bg-[var(--bg)] p-0.5">
                {(["data", "structure", "sql"] as Tab[]).map((t) => (
                  <button
                    key={t}
                    onClick={() => setTabPersist(t)}
                    className={`flex items-center gap-1 rounded px-2 py-0.5 text-[11px] capitalize font-medium transition-colors ${
                      tab === t
                        ? "bg-violet-600 text-white"
                        : "text-gray-400 hover:text-gray-200"
                    }`}
                  >
                    {t === "data" && <TableIcon size={11} />}
                    {t === "structure" && <SlidersIcon size={11} />}
                    {t === "sql" && <PlayIcon size={10} />}
                    <span>{t}</span>
                  </button>
                ))}
              </div>

              {/* Tab-specific actions */}
              {tab === "data" && sel ? (
                <div className="ml-auto flex flex-wrap items-center gap-1 text-[10px]">
                  <button
                    type="button"
                    onClick={() => setFilterBarOpen((v) => !v)}
                    className={`flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors ${
                      activeFilterWhere
                        ? "bg-violet-600 text-white font-medium shadow-xs"
                        : filterBarOpen
                          ? "bg-violet-950/60 text-violet-300 border border-violet-800"
                          : "text-gray-300 hover:bg-[var(--surface-hover)]"
                    }`}
                    data-tooltip="Filter & search rows by column or value (Ctrl+F)"
                  >
                    <FilterIcon size={11} />
                    <span>Filter</span>
                  </button>

                  <div className="mx-0.5 h-3 w-px bg-[var(--border)]" />

                  <button
                    type="button"
                    disabled={!sel || busy}
                    onClick={() => setCreateTableOpen(true)}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-violet-300 hover:bg-[var(--surface-hover)] hover:text-violet-100 disabled:opacity-30"
                    data-tooltip="Create a new table in this database"
                  >
                    <PlusIcon size={11} />
                    <span>Table</span>
                  </button>

                  <button
                    type="button"
                    disabled={!rows || busy}
                    onClick={() => setInsertOpen(true)}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip={
                      instances.find((i) => i.endpoint.id === sel.endpointId)?.endpoint
                        .engine === "redis"
                        ? "Add new Redis key"
                        : "Insert new row"
                    }
                  >
                    <PlusIcon size={11} />
                    <span>
                      {instances.find((i) => i.endpoint.id === sel.endpointId)?.endpoint
                        .engine === "redis"
                        ? "Key"
                        : "Row"}
                    </span>
                  </button>

                  <button
                    type="button"
                    disabled={!rows || busy}
                    onClick={() => void duplicateTable()}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip="Clone / duplicate this table"
                  >
                    <DuplicateIcon size={11} />
                    <span>Clone</span>
                  </button>

                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void truncateTable()}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-amber-400/90 hover:bg-amber-950/40 hover:text-amber-300 disabled:opacity-30"
                    data-tooltip="Delete all rows in this table (TRUNCATE)"
                  >
                    <EraserIcon size={11} />
                    <span>Truncate</span>
                  </button>

                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void dropTable()}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-red-400 hover:bg-red-950/50 hover:text-red-200 disabled:opacity-30"
                    data-tooltip="DROP TABLE permanently"
                  >
                    <TrashIcon size={11} />
                    <span>Drop</span>
                  </button>

                  <div className="mx-0.5 h-3 w-px bg-[var(--border)]" />

                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void importSqlFile()}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip="Import .sql dump file (up to 64 MB)"
                  >
                    <UploadIcon size={11} />
                    <span>Import</span>
                  </button>

                  <button
                    type="button"
                    disabled={!rows}
                    onClick={() =>
                      exportCsv(rows, `${sel.schema}_${sel.table}_p${page + 1}.csv`)
                    }
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip="Export current page as CSV"
                  >
                    <DownloadIcon size={11} />
                    <span>CSV</span>
                  </button>

                  <button
                    type="button"
                    disabled={busy || !sel}
                    onClick={() => void exportAllCsv()}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip="Export full table as CSV (paged)"
                  >
                    <DownloadIcon size={11} />
                    <span>All CSV</span>
                  </button>

                  <button
                    type="button"
                    disabled={!rows || (rows?.rows.length ?? 0) === 0}
                    onClick={() => exportSqlInserts(rows, `${sel.schema}.${sel.table}`)}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip="Export page as SQL INSERT dump"
                  >
                    <DownloadIcon size={11} />
                    <span>SQL</span>
                  </button>

                  <div className="mx-0.5 h-3 w-px bg-[var(--border)]" />

                  {/* Paging controls */}
                  <div className="flex items-center gap-1 bg-[var(--bg)] px-1 py-0.5 rounded border border-[var(--border)]">
                    <button
                      disabled={page === 0 || busy}
                      onClick={() => void showTable(sel, page - 1)}
                      className="rounded p-0.5 text-gray-400 hover:text-white disabled:opacity-30"
                      data-tooltip="Previous page"
                    >
                      <ChevronLeftIcon size={11} />
                    </button>
                    <span className="tabular-nums font-mono text-[10px] text-gray-300 px-1">
                      {page * pageSize + 1}GÇô{page * pageSize + (rows?.rows.length ?? 0)}
                      {tableRowCount != null ? (
                        <span className="text-gray-500"> / {tableRowCount.toLocaleString()}</span>
                      ) : null}
                    </span>
                    <button
                      disabled={(rows?.rows.length ?? 0) < pageSize || busy}
                      onClick={() => void showTable(sel, page + 1)}
                      className="rounded p-0.5 text-gray-400 hover:text-white disabled:opacity-30"
                      data-tooltip="Next page"
                    >
                      <ChevronRightIcon size={11} />
                    </button>
                  </div>

                  <select
                    className="rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[10px] text-gray-300 outline-none"
                    value={pageSize}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      setPageSize(n);
                      if (!sel) return;
                      setPage(0);
                      setBusy(true);
                      void api
                        .dbSelectPage(sel.sessionId, sel.schema, sel.table, n, 0)
                        .then((data) => setRows(data))
                        .catch((err) => setError(String(err)))
                        .finally(() => setBusy(false));
                    }}
                    data-tooltip="Rows per page"
                  >
                    {PAGE_SIZE_OPTIONS.map((n) => (
                      <option key={n} value={n}>
                        {n}/pg
                      </option>
                    ))}
                  </select>

                  <button
                    type="button"
                    className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] transition-colors ${
                      autoRefresh
                        ? "bg-emerald-950/60 text-emerald-300 border border-emerald-800/50"
                        : "text-gray-500 hover:bg-[var(--border)] hover:text-gray-300"
                    }`}
                    onClick={() => setAutoRefresh((v) => !v)}
                    data-tooltip={
                      autoRefresh
                        ? `Live auto-refresh active (${settings.refreshInterval}s) GÇö click to stop`
                        : `Auto-refresh page every ${settings.refreshInterval}s`
                    }
                  >
                    <RefreshIcon
                      size={10}
                      className={autoRefresh ? "animate-spin text-emerald-400" : ""}
                    />
                    <span>{autoRefresh ? "Live" : "Refresh"}</span>
                  </button>
                </div>
              ) : null}

              {tab === "sql" ? (
                <div className="ml-auto flex items-center gap-1 text-[10px]">
                  <button
                    type="button"
                    disabled={busy || !sel?.sessionId}
                    onClick={() => void importSqlFile()}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white disabled:opacity-30"
                    data-tooltip="Import .sql script"
                  >
                    <UploadIcon size={11} />
                    <span>Import</span>
                  </button>
                  {sqlResult ? (
                    <button
                      type="button"
                      onClick={() => exportCsv(sqlResult, "query_result.csv")}
                      className="flex items-center gap-1 rounded px-1.5 py-0.5 text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white"
                      data-tooltip="Export query result as CSV"
                    >
                      <DownloadIcon size={11} />
                      <span>CSV</span>
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>

            {/* Advanced Search & Filter Builder Bar */}
            {tab === "data" && filterBarOpen && sel && (
              <div className="flex flex-wrap items-center gap-1.5 border-b border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1.5 text-[11px] select-none animate-in fade-in duration-100">
                <div className="flex items-center gap-1 text-violet-400 font-medium shrink-0">
                  <FilterIcon size={12} />
                  <span>Search Rows:</span>
                </div>

                {/* Column dropdown */}
                <select
                  value={filterCol}
                  onChange={(e) => setFilterCol(e.target.value)}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-200 outline-none focus:border-violet-500"
                >
                  <option value="">All Columns</option>
                  {columns.map((c) => (
                    <option key={c.name} value={c.name}>
                      {c.name} {c.primary ? "(PK)" : ""}
                    </option>
                  ))}
                </select>

                {/* Operator dropdown */}
                <select
                  value={filterOp}
                  onChange={(e) => setFilterOp(e.target.value)}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-200 outline-none focus:border-violet-500"
                >
                  <option value="contains">contains</option>
                  <option value="equals">equals (=)</option>
                  <option value="starts">starts with</option>
                  <option value="ends">ends with</option>
                  <option value="neq">not equals (!=)</option>
                  <option value="gt">&gt; greater</option>
                  <option value="lt">&lt; less</option>
                  <option value="gte">&gt;=</option>
                  <option value="lte">&lt;=</option>
                  <option value="null">IS NULL</option>
                  <option value="notnull">IS NOT NULL</option>
                </select>

                {/* Value input */}
                {filterOp !== "null" && filterOp !== "notnull" && (
                  <div className="relative flex min-w-[140px] flex-1 items-center">
                    <input
                      type="text"
                      autoFocus
                      value={filterVal}
                      onChange={(e) => setFilterVal(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") applyFilter();
                        if (e.key === "Escape") setFilterBarOpen(false);
                      }}
                      placeholder="Value to searchGÇª"
                      className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
                    />
                  </div>
                )}

                <button
                  type="button"
                  onClick={applyFilter}
                  disabled={busy}
                  className="flex items-center gap-1 rounded bg-violet-600 px-2.5 py-0.5 font-medium text-white hover:bg-violet-500 disabled:opacity-40 transition-colors"
                >
                  <SearchIcon size={10} />
                  <span>Search</span>
                </button>

                {activeFilterWhere && (
                  <button
                    type="button"
                    onClick={clearFilter}
                    className="flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-gray-400 hover:text-white"
                  >
                    <CloseIcon size={10} />
                    <span>Clear Filter</span>
                  </button>
                )}

                <button
                  type="button"
                  onClick={() => setFilterBarOpen(false)}
                  className="ml-auto rounded p-0.5 text-gray-500 hover:text-white"
                  data-tooltip="Hide search bar"
                >
                  <CloseIcon size={12} />
                </button>
              </div>
            )}

            {/* Quick Horizontal Table Tabs Strip */}
            {showTableTabsBar && sel && (
              <div className="flex shrink-0 items-center gap-1 border-b border-[var(--border)] bg-[var(--bg)] px-2 py-1 overflow-x-auto select-none">
                <div className="flex items-center gap-1 text-[10px] font-semibold text-violet-400 shrink-0 pr-1">
                  <DatabaseIcon size={11} />
                  <span>{sel.schema}</span>
                </div>

                <div className="relative flex items-center shrink-0 w-28">
                  <SearchIcon
                    size={9}
                    className="absolute left-1.5 text-gray-500 pointer-events-none"
                  />
                  <input
                    type="text"
                    value={tableTabsFilter}
                    onChange={(e) => setTableTabsFilter(e.target.value)}
                    placeholder="FilterGÇª"
                    className="w-full rounded border border-[var(--border)] bg-[var(--surface)] pl-4 pr-1 py-0.5 text-[9px] text-gray-200 outline-none focus:border-violet-500 placeholder:text-gray-600"
                  />
                </div>

                <div className="flex items-center gap-1 overflow-x-auto min-w-0 flex-1 py-0.5">
                  {filteredSchemaTables.map((t) => {
                    const active = sel.table === t.name;
                    return (
                      <button
                        key={t.name}
                        onClick={() => {
                          if (activeInstance) {
                            openTable({
                              endpointId: activeInstance.endpoint.id,
                              sessionId: activeInstance.sessionId ?? sel.sessionId,
                              schema: sel.schema,
                              table: t.name,
                            });
                          }
                        }}
                        className={`flex items-center gap-1 rounded px-2 py-0.5 text-[10.5px] font-mono whitespace-nowrap transition-all ${
                          active
                            ? "bg-violet-600 text-white font-medium shadow-xs"
                            : "bg-[var(--surface)] text-gray-400 hover:bg-[var(--surface-hover)] hover:text-gray-200 border border-[var(--border)]"
                        }`}
                        title={`${t.name} -+ ${t.rows.toLocaleString()} rows`}
                      >
                        <TableIcon size={10} className={active ? "text-white" : "text-gray-500"} />
                        <span>{t.name}</span>
                        {t.rows > 0 && (
                          <span
                            className={`rounded px-1 text-[8.5px] tabular-nums ${
                              active
                                ? "bg-violet-800 text-violet-200"
                                : "bg-[var(--bg)] text-gray-500"
                            }`}
                          >
                            {t.rows >= 1000 ? `${(t.rows / 1000).toFixed(0)}k` : t.rows}
                          </span>
                        )}
                      </button>
                    );
                  })}
                  {filteredSchemaTables.length === 0 && (
                    <span className="text-[10px] text-gray-500 italic px-2">
                      No matching tables
                    </span>
                  )}
                </div>

                <button
                  type="button"
                  onClick={() => setCreateTableOpen(true)}
                  className="shrink-0 rounded p-1 text-gray-500 hover:bg-[var(--border)] hover:text-violet-300"
                  data-tooltip="Create new table"
                >
                  <PlusIcon size={11} />
                </button>
              </div>
            )}

            {/* Content Area */}
            <div className="min-h-0 flex-1 overflow-hidden">
              {tab === "data" ? (
                rows ? (
                  <Grid
                    set={rows}
                    columns={columns}
                    settings={settings}
                    tableLabel={sel ? `${sel.schema}.${sel.table}` : undefined}
                    totalRows={tableRowCount}
                    activeFilter={activeFilterWhere}
                    onClearFilter={clearFilter}
                    onEdit={(r, c, v) => void editCell(r, c, v)}
                    onDeleteRows={(idx) => void deleteRows(idx)}
                    onSqlTemplate={(text) => {
                      setSql(text);
                      setTab("sql");
                    }}
                    onInspectRow={(idx) => setInspectRowIndex(idx)}
                  />
                ) : (
                  <div className="flex h-full flex-col items-center justify-center p-6 text-center text-gray-500">
                    <DatabaseIcon size={28} className="text-violet-500/40 mb-2" />
                    <p className="text-[12px] font-medium text-gray-300">No table selected</p>
                    <p className="text-[11px] text-gray-500 max-w-sm mt-1">
                      Expand a database, sign in, and click any table to browse its data.
                    </p>
                  </div>
                )
              ) : null}

              {tab === "structure" ? (
                columns.length > 0 ? (
                  <div className="flex h-full min-h-0 flex-col">
                    <div className="flex shrink-0 items-center gap-2 border-b border-[var(--border)] bg-[var(--surface-2)] px-2 py-1">
                      <button
                        type="button"
                        className="flex items-center gap-1 rounded bg-violet-600/20 px-2 py-0.5 text-[10px] font-medium text-violet-300 hover:bg-violet-600/30"
                        data-tooltip="Add column to this table"
                        onClick={() => setAddColumnOpen(true)}
                      >
                        <PlusIcon size={11} />
                        <span>Add Column</span>
                      </button>
                      <button
                        type="button"
                        className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"
                        data-tooltip="Copy column definitions as CREATE TABLE SQL"
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
                        <CopyIcon size={11} />
                        <span>Copy CREATE</span>
                      </button>
                      <button
                        type="button"
                        className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"
                        data-tooltip="Copy column names list"
                        onClick={() => {
                          void navigator.clipboard.writeText(
                            columns.map((c) => c.name).join(", "),
                          );
                        }}
                      >
                        <CopyIcon size={11} />
                        <span>Copy Names</span>
                      </button>
                      <span className="ml-auto font-mono text-[10px] text-gray-500">
                        {columns.length} columns
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
                        settings={settings}
                      />
                    </div>
                  </div>
                ) : (
                  <p className="p-3 text-[11px] text-gray-500">Select a table on the left.</p>
                )
              ) : null}

              {tab === "sql" ? (
                <div className="flex h-full flex-col">
                  {/* Editor */}
                  <div className="h-1/2 min-h-0 p-1">
                    <CodeEditArea
                      value={sql}
                      onChange={setSql}
                      path="query.sql"
                      onKeyDown={(e) => {
                        if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                          e.preventDefault();
                          void runSql();
                          return true;
                        }
                        return false;
                      }}
                    />
                  </div>

                  {/* SQL Action Bar */}
                  <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-y border-[var(--border)] bg-[var(--surface-2)] px-2 py-1 text-[11px]">
                    <button
                      onClick={() => void runSql()}
                      disabled={busy}
                      className="flex items-center gap-1.5 rounded bg-violet-600 px-2.5 py-0.5 font-medium text-white hover:bg-violet-500 disabled:opacity-50 transition-colors"
                      data-tooltip="Run query (Ctrl+Enter)"
                    >
                      <PlayIcon size={11} />
                      <span>{busy ? "RunningGÇª" : "Run"}</span>
                    </button>

                    <button
                      type="button"
                      disabled={busy || !sql.trim() || !sel?.sessionId}
                      onClick={() => {
                        void (async () => {
                          if (!sel?.sessionId || !sql.trim()) return;
                          setBusy(true);
                          setError(null);
                          try {
                            const q = sql.trim().replace(/;+\s*$/, "");
                            const result = await api.dbRunSql(
                              sel.sessionId,
                              `EXPLAIN ${q}`,
                            );
                            setSqlResult(result);
                          } catch (e) {
                            setError(String(e));
                          } finally {
                            setBusy(false);
                          }
                        })();
                      }}
                      className="rounded border border-[var(--border)] px-2 py-0.5 text-gray-300 hover:bg-[var(--border)] disabled:opacity-40"
                      data-tooltip="Explain execution plan"
                    >
                      Explain
                    </button>

                    <button
                      type="button"
                      disabled={!sql.trim()}
                      onClick={() => {
                        const kw =
                          /\b(select|from|where|and|or|join|left|right|inner|outer|on|group by|order by|limit|offset|insert into|values|update|set|delete|create|table|alter|drop|as|in|not|null|is|like|between|union|all|distinct|having|case|when|then|else|end)\b/gi;
                        let s = sql
                          .replace(/\r\n/g, "\n")
                          .replace(/[ \t]+/g, " ")
                          .replace(/ *\n */g, "\n")
                          .trim();
                        s = s.replace(kw, (m) => m.toUpperCase());
                        s = s
                          .replace(/\bFROM\b/g, "\nFROM")
                          .replace(/\bWHERE\b/g, "\nWHERE")
                          .replace(/\b(AND|OR)\b/g, "\n  $1")
                          .replace(/\bJOIN\b/g, "\nJOIN")
                          .replace(/\bLEFT JOIN\b/g, "\nLEFT JOIN")
                          .replace(/\bGROUP BY\b/g, "\nGROUP BY")
                          .replace(/\bORDER BY\b/g, "\nORDER BY")
                          .replace(/\bLIMIT\b/g, "\nLIMIT")
                          .replace(/\bVALUES\b/g, "\nVALUES")
                          .replace(/\bSET\b/g, "\nSET");
                        setSql(s.trim() + (s.trim().endsWith(";") ? "" : ";"));
                      }}
                      className="flex items-center gap-1 rounded border border-[var(--border)] px-2 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 disabled:opacity-40"
                      data-tooltip="Format SQL"
                    >
                      <SlidersIcon size={11} />
                      <span>Format</span>
                    </button>

                    <div className="mx-0.5 h-3 w-px bg-[var(--border)]" />

                    {/* Quick Snippets */}
                    {sel && (
                      <div className="flex items-center gap-1">
                        <button
                          type="button"
                          onClick={() =>
                            setSql(`SELECT * FROM ${sel.schema}.${sel.table} LIMIT 100;`)
                          }
                          className="rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-300 hover:text-white"
                          data-tooltip="Insert SELECT * template"
                        >
                          SELECT
                        </button>
                        <button
                          type="button"
                          onClick={() =>
                            setSql(
                              `SELECT COUNT(*) AS total FROM ${sel.schema}.${sel.table};`,
                            )
                          }
                          className="rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-300 hover:text-white"
                          data-tooltip="Insert COUNT template"
                        >
                          COUNT
                        </button>
                      </div>
                    )}

                    <button
                      type="button"
                      onClick={toggleFavorite}
                      disabled={!sql.trim()}
                      className="rounded p-1 disabled:opacity-30 text-amber-300 hover:bg-[var(--border)]"
                      data-tooltip={
                        sqlFavorites.includes(sql.trim())
                          ? "Remove from favorites"
                          : "Save query to favorites"
                      }
                    >
                      {sqlFavorites.includes(sql.trim()) ? (
                        <StarFilledIcon size={12} />
                      ) : (
                        <StarOutlineIcon size={12} />
                      )}
                    </button>

                    {sqlFavorites.length > 0 && (
                      <select
                        className="max-w-[180px] rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[10px] text-gray-300 outline-none"
                        defaultValue=""
                        onChange={(e) => {
                          const v = e.target.value;
                          e.target.value = "";
                          if (!v) return;
                          setSql(v);
                        }}
                        data-tooltip="Favorites"
                      >
                        <option value="" disabled>
                          Gÿà Favorites ({sqlFavorites.length})
                        </option>
                        {sqlFavorites.map((q, i) => (
                          <option key={`f-${i}`} value={q}>
                            {q.length > 50 ? `${q.slice(0, 50)}GÇª` : q}
                          </option>
                        ))}
                      </select>
                    )}

                    {sqlHistory.length > 0 && (
                      <select
                        className="max-w-[180px] rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[10px] text-gray-300 outline-none"
                        defaultValue=""
                        onChange={(e) => {
                          if (e.target.value) setSql(e.target.value);
                          e.target.value = "";
                        }}
                        data-tooltip="Query history"
                      >
                        <option value="" disabled>
                          History ({sqlHistory.length})
                        </option>
                        {sqlHistory.map((q, i) => (
                          <option key={i} value={q}>
                            {q.length > 60 ? `${q.slice(0, 60)}GÇª` : q}
                          </option>
                        ))}
                      </select>
                    )}

                    {sqlResult?.message ? (
                      <span className="truncate text-[10px] text-amber-300">
                        {sqlResult.message}
                      </span>
                    ) : null}

                    {sqlResult ? (
                      <span className="ml-auto shrink-0 font-mono text-[10px] tabular-nums text-gray-400">
                        {sqlResult.rows.length} row{sqlResult.rows.length === 1 ? "" : "s"}
                        {sqlResult.affected != null
                          ? ` -+ ${sqlResult.affected} affected`
                          : ""}
                      </span>
                    ) : null}
                  </div>

                  {/* SQL Results Grid */}
                  <div className="min-h-0 flex-1 overflow-hidden">
                    {sqlResult ? (
                      <Grid
                        set={sqlResult}
                        settings={settings}
                        onInspectRow={(idx) => setInspectRowIndex(idx)}
                      />
                    ) : (
                      <p className="p-3 text-[11px] text-gray-500">
                        Write a statement and press Run (Ctrl+Enter).
                      </p>
                    )}
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        </div>
      </div>

      {/* Settings Modal */}
      <DbSettingsModal
        open={settingsOpen}
        vpsId={data.vpsId}
        onClose={() => setSettingsOpen(false)}
        onSavedLoginsChanged={() => void refreshSaved()}
        onSettingsChanged={(newSettings) => setSettings(newSettings)}
      />

      {/* Row Inspector Modal */}
      {inspectRowIndex !== null && (tab === "data" ? rows : sqlResult) ? (
        <RowInspectorModal
          open={inspectRowIndex !== null}
          rowIndex={inspectRowIndex}
          set={tab === "data" ? rows! : sqlResult!}
          columns={tab === "data" ? columns : undefined}
          tableName={sel ? `${sel.schema}.${sel.table}` : undefined}
          editable={tab === "data" && columns.some((c) => c.primary)}
          onClose={() => setInspectRowIndex(null)}
          onSelectRowIndex={(idx) => setInspectRowIndex(idx)}
          onSaveRow={handleSaveRowInspector}
        />
      ) : null}

      {/* Insert Row Modal */}
      {insertOpen && sel ? (
        <InsertRowModal
          open={insertOpen}
          schema={sel.schema}
          table={sel.table}
          columns={columns}
          isRedis={
            instances.find((i) => i.endpoint.id === sel.endpointId)?.endpoint
              .engine === "redis"
          }
          onClose={() => setInsertOpen(false)}
          onSubmitSql={(q) => api.dbRunSql(sel.sessionId, q)}
          onSuccess={() => {
            void showTable(sel, page);
          }}
        />
      ) : null}

      {/* Create Table Modal */}
      {createTableOpen && sel ? (
        <CreateTableModal
          open={createTableOpen}
          schema={sel.schema}
          engine={
            (instances.find((i) => i.endpoint.id === sel.endpointId)?.endpoint
              .engine as any) ?? "mysql"
          }
          onClose={() => setCreateTableOpen(false)}
          onSubmitSql={(q) => api.dbRunSql(sel.sessionId, q)}
          onSuccess={(newTable) => {
            void (async () => {
              try {
                const updated = await api.dbListTables(sel.sessionId, sel.schema);
                const cur = instances.find((i) => i.endpoint.id === sel.endpointId);
                patch(sel.endpointId, {
                  tables: {
                    ...(cur?.tables ?? {}),
                    [sel.schema]: updated,
                  },
                });
                openTable({
                  endpointId: sel.endpointId,
                  sessionId: sel.sessionId,
                  schema: sel.schema,
                  table: newTable,
                });
              } catch (e) {
                setError(String(e));
              }
            })();
          }}
        />
      ) : null}

      {/* Add Column Modal */}
      {addColumnOpen && sel ? (
        <AddColumnModal
          open={addColumnOpen}
          schema={sel.schema}
          table={sel.table}
          existingColumns={columns}
          engine={
            (instances.find((i) => i.endpoint.id === sel.endpointId)?.endpoint
              .engine as any) ?? "mysql"
          }
          onClose={() => setAddColumnOpen(false)}
          onSubmitSql={(q) => api.dbRunSql(sel.sessionId, q)}
          onSuccess={() => {
            void showTable(sel, page);
          }}
        />
      ) : null}

      {/* Redis Key Inspect Modal */}
      {redisKeyInspect && sel ? (
        <RedisKeyModal
          open={Boolean(redisKeyInspect)}
          sessionId={sel.sessionId}
          schema={sel.schema}
          keyName={redisKeyInspect.key}
          initialType={redisKeyInspect.type}
          initialTtl={redisKeyInspect.ttl}
          onClose={() => setRedisKeyInspect(null)}
          onSubmitSql={(q) => api.dbRunSql(sel.sessionId, q)}
          onSuccess={() => {
            void showTable(sel, page);
          }}
        />
      ) : null}
    </div>
  );
});
