import { useState } from "react";
import type { DbColumn, DbResultSet } from "../../lib/tauri";
import {
  CloseIcon,
  CopyIcon,
  CheckIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  InfoIcon,
} from "../icons";

interface RowInspectorModalProps {
  open: boolean;
  rowIndex: number;
  set: DbResultSet;
  columns?: DbColumn[];
  tableName?: string;
  onClose: () => void;
  onSelectRowIndex?: (index: number) => void;
}

export function RowInspectorModal({
  open,
  rowIndex,
  set,
  columns,
  tableName,
  onClose,
  onSelectRowIndex,
}: RowInspectorModalProps) {
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  if (!open || !set.rows[rowIndex]) return null;

  const row = set.rows[rowIndex];
  const canPrev = rowIndex > 0;
  const canNext = rowIndex < set.rows.length - 1;

  const copyToClipboard = (text: string, key: string) => {
    void navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 1500);
  };

  const copyRowJson = () => {
    const obj: Record<string, string | null> = {};
    set.columns.forEach((c, i) => {
      obj[c] = row[i];
    });
    copyToClipboard(JSON.stringify(obj, null, 2), "__all_json__");
  };

  const copyRowSql = () => {
    const cols = set.columns.join(", ");
    const vals = row
      .map((v) => {
        if (v === null) return "NULL";
        return `'${String(v).replace(/'/g, "''")}'`;
      })
      .join(", ");
    const tbl = tableName || "table_name";
    copyToClipboard(`INSERT INTO ${tbl} (${cols}) VALUES (${vals});`, "__all_sql__");
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-xs p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex w-full max-w-2xl max-h-[85vh] flex-col overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-2xl text-[12px] animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-4 py-2.5">
          <div className="flex items-center gap-2 font-medium text-gray-200">
            <InfoIcon size={15} className="text-violet-400" />
            <span>Row Inspector</span>
            {tableName && (
              <span className="font-mono text-[11px] text-violet-300">
                · {tableName}
              </span>
            )}
            <span className="rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] font-mono text-gray-400">
              Row {rowIndex + 1} of {set.rows.length}
            </span>
          </div>

          <div className="flex items-center gap-1">
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
              data-tooltip="Close"
            >
              <CloseIcon size={14} />
            </button>
          </div>
        </div>

        {/* Action bar */}
        <div className="flex items-center gap-2 border-b border-[var(--border)] bg-[var(--bg)] px-3 py-1.5">
          <button
            type="button"
            onClick={copyRowJson}
            className="flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-[11px] text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white"
          >
            {copiedKey === "__all_json__" ? (
              <CheckIcon size={12} className="text-emerald-400" />
            ) : (
              <CopyIcon size={12} />
            )}
            <span>Copy Row JSON</span>
          </button>
          <button
            type="button"
            onClick={copyRowSql}
            className="flex items-center gap-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-[11px] text-gray-300 hover:bg-[var(--surface-hover)] hover:text-white"
          >
            {copiedKey === "__all_sql__" ? (
              <CheckIcon size={12} className="text-emerald-400" />
            ) : (
              <CopyIcon size={12} />
            )}
            <span>Copy SQL INSERT</span>
          </button>
        </div>

        {/* Fields list */}
        <div className="min-h-0 flex-1 overflow-y-auto p-4 space-y-2.5">
          {set.columns.map((colName, idx) => {
            const val = row[idx];
            const meta = columns?.find((c) => c.name === colName);
            const isPk = meta?.primary;
            const isNull = val === null;

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
                className="group rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-2.5 transition-colors hover:border-violet-500/40"
              >
                <div className="flex items-center justify-between gap-2 mb-1">
                  <div className="flex items-center gap-1.5">
                    {isPk && (
                      <span
                        className="rounded bg-amber-950/70 px-1 py-0.2 text-[9px] font-mono text-amber-400 border border-amber-800/40"
                        title="Primary Key"
                      >
                        PRI
                      </span>
                    )}
                    <span className="font-mono font-medium text-gray-200">{colName}</span>
                    {meta?.data_type && (
                      <span className="font-mono text-[10px] text-gray-500">
                        ({meta.data_type})
                      </span>
                    )}
                  </div>
                  <button
                    type="button"
                    onClick={() => copyToClipboard(val ?? "NULL", colName)}
                    className="opacity-0 group-hover:opacity-100 flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 transition-opacity"
                    data-tooltip="Copy field value"
                  >
                    {copiedKey === colName ? (
                      <CheckIcon size={11} className="text-emerald-400" />
                    ) : (
                      <CopyIcon size={11} />
                    )}
                    <span>Copy</span>
                  </button>
                </div>

                <div className="rounded border border-[var(--border)] bg-[var(--bg)] p-2">
                  {isNull ? (
                    <span className="italic text-gray-500 font-mono text-[11px]">NULL</span>
                  ) : isJson ? (
                    <pre className="font-mono text-[11px] text-emerald-300 max-h-48 overflow-auto whitespace-pre-wrap break-all">
                      {formattedJson}
                    </pre>
                  ) : (
                    <div className="font-mono text-[11px] text-gray-200 whitespace-pre-wrap break-all select-text">
                      {val}
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] bg-[var(--surface-2)] px-4 py-2">
          <span className="text-[11px] text-gray-500">
            {set.columns.length} column{set.columns.length === 1 ? "" : "s"}
          </span>
          <button
            onClick={onClose}
            className="rounded bg-[var(--border)] px-3 py-1 text-[11px] text-gray-200 hover:bg-violet-600 hover:text-white transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
