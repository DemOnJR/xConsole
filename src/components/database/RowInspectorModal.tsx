import { useEffect, useMemo, useState } from "react";
import type { DbColumn, DbResultSet } from "../../lib/tauri";
import {
  CloseIcon,
  CopyIcon,
  CheckIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  SearchIcon,
  SlidersIcon,
  RefreshIcon,
  InfoIcon,
} from "../icons";

interface RowInspectorModalProps {
  open: boolean;
  rowIndex: number;
  set: DbResultSet;
  columns?: DbColumn[];
  tableName?: string;
  editable?: boolean;
  onClose: () => void;
  onSelectRowIndex?: (index: number) => void;
  onSaveRow?: (rowIndex: number, updatedValues: Record<string, string | null>) => Promise<void> | void;
}

export function RowInspectorModal({
  open,
  rowIndex,
  set,
  columns,
  tableName,
  editable = true,
  onClose,
  onSelectRowIndex,
  onSaveRow,
}: RowInspectorModalProps) {
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [fieldFilter, setFieldFilter] = useState("");
  const [drafts, setDrafts] = useState<Record<string, string | null>>({});
  const [saving, setSaving] = useState(false);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [jumpIndexText, setJumpIndexText] = useState("");

  const row = set.rows[rowIndex];
  const canPrev = rowIndex > 0;
  const canNext = rowIndex < set.rows.length - 1;

  // Initialize draft values whenever row or rowIndex changes
  useEffect(() => {
    if (!row) return;
    const initial: Record<string, string | null> = {};
    set.columns.forEach((c, i) => {
      initial[c] = row[i];
    });
    setDrafts(initial);
    setJumpIndexText(String(rowIndex + 1));
  }, [row, rowIndex, set.columns]);

  // Check if any draft value was modified
  const modifiedColumns = useMemo(() => {
    if (!row) return [];
    const modified: string[] = [];
    set.columns.forEach((colName, i) => {
      const originalVal = row[i];
      const currentDraft = drafts[colName];
      if (currentDraft !== originalVal) {
        modified.push(colName);
      }
    });
    return modified;
  }, [row, drafts, set.columns]);

  const isDirty = modifiedColumns.length > 0;

  if (!open || !set.rows[rowIndex]) return null;

  const copyToClipboard = (text: string, key: string) => {
    void navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 1500);
  };

  const copyRowJson = () => {
    copyToClipboard(JSON.stringify(drafts, null, 2), "__all_json__");
  };

  const copyRowSql = () => {
    const cols = set.columns.join(", ");
    const vals = set.columns
      .map((c) => {
        const v = drafts[c];
        if (v === null) return "NULL";
        return `'${String(v).replace(/'/g, "''")}'`;
      })
      .join(", ");
    const tbl = tableName || "table_name";
    copyToClipboard(`INSERT INTO ${tbl} (${cols}) VALUES (${vals});`, "__all_sql__");
  };

  const handleSave = async () => {
    if (!onSaveRow || !isDirty || saving) return;
    setSaving(true);
    try {
      await onSaveRow(rowIndex, drafts);
      setSaveSuccess(true);
      setTimeout(() => setSaveSuccess(false), 2000);
    } catch {
      /* handled in parent */
    } finally {
      setSaving(false);
    }
  };

  const handleRevert = () => {
    const initial: Record<string, string | null> = {};
    set.columns.forEach((c, i) => {
      initial[c] = row[i];
    });
    setDrafts(initial);
  };

  const filteredColumns = useMemo(() => {
    if (!fieldFilter.trim()) return set.columns;
    const q = fieldFilter.trim().toLowerCase();
    return set.columns.filter((c) => {
      const val = String(drafts[c] ?? "");
      return c.toLowerCase().includes(q) || val.toLowerCase().includes(q);
    });
  }, [set.columns, fieldFilter, drafts]);

  return (
    <div
      className="nowheel nopan fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-xs p-4"
      onWheel={(e) => e.stopPropagation()}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      onKeyDown={(e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
          e.preventDefault();
          void handleSave();
        }
      }}
    >
      <div
        className="nowheel nopan flex w-full max-w-2xl max-h-[88vh] flex-col overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-2xl text-[12px] animate-in fade-in zoom-in-95 duration-150"
        onWheel={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-4 py-2.5 select-none">
          <div className="flex items-center gap-2 font-medium text-gray-200">
            <InfoIcon size={15} className="text-violet-400" />
            <span>Row Inspector & Editor</span>
            {tableName && (
              <span className="font-mono text-[11px] text-violet-300">
                · {tableName}
              </span>
            )}
          </div>

          <div className="flex items-center gap-1.5">
            {/* Jump to row */}
            <div className="flex items-center gap-1 bg-[var(--bg)] px-1.5 py-0.5 rounded border border-[var(--border)] text-[10px] text-gray-400 font-mono">
              <span>Row</span>
              <input
                type="text"
                value={jumpIndexText}
                onChange={(e) => setJumpIndexText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    const parsed = parseInt(jumpIndexText, 10);
                    if (!isNaN(parsed) && parsed >= 1 && parsed <= set.rows.length) {
                      onSelectRowIndex?.(parsed - 1);
                    }
                  }
                }}
                className="w-8 text-center bg-[var(--surface)] text-gray-100 rounded border border-transparent focus:border-violet-500 outline-none"
              />
              <span>/ {set.rows.length}</span>
            </div>

            <button
              onClick={() => onSelectRowIndex?.(rowIndex - 1)}
              disabled={!canPrev}
              className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-white disabled:opacity-30"
              data-tooltip="Previous row"
            >
              <ChevronLeftIcon size={14} />
            </button>
            <button
              onClick={() => onSelectRowIndex?.(rowIndex + 1)}
              disabled={!canNext}
              className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-white disabled:opacity-30"
              data-tooltip="Next row"
            >
              <ChevronRightIcon size={14} />
            </button>
            <div className="mx-1 h-3.5 w-px bg-[var(--border)]" />
            <button
              onClick={onClose}
              className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-white"
              data-tooltip="Close (Esc)"
            >
              <CloseIcon size={14} />
            </button>
          </div>
        </div>

        {/* Action bar & Search */}
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] bg-[var(--bg)] px-3 py-1.5 select-none">
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={copyRowJson}
              className="flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-[11px] text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white"
              data-tooltip="Copy entire row as JSON"
            >
              {copiedKey === "__all_json__" ? (
                <CheckIcon size={12} className="text-emerald-400" />
              ) : (
                <CopyIcon size={12} />
              )}
              <span>JSON</span>
            </button>
            <button
              type="button"
              onClick={copyRowSql}
              className="flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-[11px] text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white"
              data-tooltip="Copy as SQL INSERT statement"
            >
              {copiedKey === "__all_sql__" ? (
                <CheckIcon size={12} className="text-emerald-400" />
              ) : (
                <CopyIcon size={12} />
              )}
              <span>SQL</span>
            </button>

            {isDirty && (
              <span className="font-mono text-[10px] text-amber-400 bg-amber-950/50 px-1.5 py-0.5 rounded border border-amber-800/50">
                {modifiedColumns.length} modified
              </span>
            )}
          </div>

          {/* Quick filter in row */}
          <div className="relative flex items-center w-52">
            <SearchIcon size={11} className="absolute left-2 text-gray-500 pointer-events-none" />
            <input
              type="text"
              value={fieldFilter}
              onChange={(e) => setFieldFilter(e.target.value)}
              placeholder="Filter columns or values…"
              className="w-full rounded border border-[var(--border)] bg-[var(--surface)] pl-6 pr-2 py-0.5 text-[11px] text-gray-200 outline-none focus:border-violet-500 placeholder:text-gray-600"
            />
          </div>
        </div>

        {/* Fields list - Scrollable */}
        <div
          className="nowheel nopan min-h-0 flex-1 overflow-y-auto p-4 space-y-3"
          onWheel={(e) => e.stopPropagation()}
        >
          {filteredColumns.map((colName) => {
            const val = drafts[colName];
            const originalVal = row[set.columns.indexOf(colName)];
            const meta = columns?.find((c) => c.name === colName);
            const isPk = meta?.primary;
            const isNull = val === null;
            const isModified = val !== originalVal;

            // Check if value is JSON
            let isJson = false;
            let formattedJson = "";
            if (val && typeof val === "string" && (val.startsWith("{") || val.startsWith("["))) {
              try {
                const parsed = JSON.parse(val);
                formattedJson = JSON.stringify(parsed, null, 2);
                isJson = true;
              } catch {
                /* not json */
              }
            }

            return (
              <div
                key={colName}
                className={`group rounded-lg border p-3 transition-colors ${
                  isModified
                    ? "border-amber-500/70 bg-amber-950/15"
                    : "border-[var(--border)] bg-[var(--surface-2)] hover:border-violet-500/40"
                }`}
              >
                <div className="flex items-center justify-between gap-2 mb-1.5 select-none">
                  <div className="flex items-center gap-1.5">
                    {isPk && (
                      <span
                        className="rounded bg-amber-950/70 px-1.5 py-0.2 text-[9px] font-mono text-amber-400 border border-amber-800/40"
                        title="Primary Key"
                      >
                        PRI
                      </span>
                    )}
                    <span className="font-mono font-medium text-gray-200 text-[11.5px]">
                      {colName}
                    </span>
                    {meta?.data_type && (
                      <span className="font-mono text-[10px] text-gray-500">
                        ({meta.data_type})
                      </span>
                    )}
                    {isModified && (
                      <span className="rounded bg-amber-500/20 px-1 py-0.2 text-[9px] font-mono text-amber-300">
                        Modified
                      </span>
                    )}
                  </div>

                  <div className="flex items-center gap-1">
                    {editable && (
                      <button
                        type="button"
                        onClick={() => {
                          setDrafts((prev) => ({
                            ...prev,
                            [colName]: isNull ? "" : null,
                          }));
                        }}
                        className={`rounded px-1.5 py-0.5 text-[10px] font-mono transition-colors ${
                          isNull
                            ? "bg-violet-600/30 text-violet-300 hover:bg-violet-600/50"
                            : "bg-[var(--surface)] text-gray-400 hover:text-amber-300 border border-[var(--border)]"
                        }`}
                        data-tooltip={isNull ? "Set string value" : "Set value to NULL"}
                      >
                        {isNull ? "Set Value" : "NULL"}
                      </button>
                    )}

                    {isJson && !isNull && (
                      <button
                        type="button"
                        onClick={() => {
                          setDrafts((prev) => ({
                            ...prev,
                            [colName]: formattedJson,
                          }));
                        }}
                        className="rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-emerald-300 hover:bg-[var(--surface-hover)]"
                        data-tooltip="Format JSON"
                      >
                        <SlidersIcon size={10} />
                      </button>
                    )}

                    <button
                      type="button"
                      onClick={() => copyToClipboard(val ?? "NULL", colName)}
                      className="opacity-0 group-hover:opacity-100 flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[10px] text-gray-400 hover:text-white transition-opacity"
                      data-tooltip="Copy field value"
                    >
                      {copiedKey === colName ? (
                        <CheckIcon size={10} className="text-emerald-400" />
                      ) : (
                        <CopyIcon size={10} />
                      )}
                      <span>Copy</span>
                    </button>
                  </div>
                </div>

                {/* Field input / editor */}
                <div className="rounded border border-[var(--border)] bg-[var(--bg)] p-2">
                  {isNull ? (
                    <div className="italic text-gray-600 font-mono text-[11px] py-1 select-none">
                      NULL
                    </div>
                  ) : editable ? (
                    <textarea
                      rows={
                        isJson || (val && val.length > 80) || (val && val.includes("\n"))
                          ? Math.min(8, Math.max(3, val.split("\n").length))
                          : 1
                      }
                      value={val ?? ""}
                      onChange={(e) => {
                        const next = e.target.value;
                        setDrafts((prev) => ({ ...prev, [colName]: next }));
                      }}
                      className="w-full resize-y bg-transparent font-mono text-[11px] text-gray-200 outline-none select-text"
                      spellCheck={false}
                    />
                  ) : (
                    <div className="font-mono text-[11px] text-gray-200 whitespace-pre-wrap break-all select-text">
                      {val}
                    </div>
                  )}
                </div>
              </div>
            );
          })}

          {filteredColumns.length === 0 && (
            <p className="py-6 text-center text-gray-500 text-[11px]">
              No columns match "{fieldFilter}"
            </p>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] bg-[var(--surface-2)] px-4 py-2.5 select-none">
          <div className="flex items-center gap-2">
            <span className="text-[11px] text-gray-500 font-mono">
              {set.columns.length} columns
            </span>
            {isDirty && (
              <button
                type="button"
                onClick={handleRevert}
                className="text-[11px] text-gray-400 hover:text-amber-300 underline"
              >
                Revert Changes
              </button>
            )}
          </div>

          <div className="flex items-center gap-2">
            {editable && (
              <button
                type="button"
                onClick={() => void handleSave()}
                disabled={!isDirty || saving}
                className="flex items-center gap-1.5 rounded bg-violet-600 px-3.5 py-1 font-medium text-white hover:bg-violet-500 disabled:opacity-40 shadow-xs transition-colors"
                data-tooltip="Save changes to database (Ctrl+S)"
              >
                {saving ? (
                  <RefreshIcon size={12} className="animate-spin" />
                ) : saveSuccess ? (
                  <CheckIcon size={12} className="text-emerald-300" />
                ) : (
                  <CheckIcon size={12} />
                )}
                <span>{saving ? "Saving…" : saveSuccess ? "Saved!" : "Save Changes"}</span>
              </button>
            )}

            <button
              onClick={onClose}
              className="rounded border border-[var(--border)] bg-[var(--surface)] px-3 py-1 text-[11px] text-gray-300 hover:bg-[var(--border)] hover:text-white transition-colors"
            >
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
