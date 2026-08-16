import { useEffect, useMemo, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { GoalTask } from "../lib/tauri";
import { goalTaskChildren } from "../lib/goalParse";

const KIND_COLOR: Record<string, string> = {
  edit: "text-amber-300",
  test: "text-emerald-300",
  bug: "text-red-300",
  research: "text-blue-300",
  check: "text-cyan-300",
};

const COL_TONE: Record<string, string> = {
  backlog: "text-gray-300 border-gray-500/40 bg-gray-500/10",
  in_progress: "text-amber-200 border-amber-500/40 bg-amber-500/10",
  waiting: "text-blue-300 border-blue-500/40 bg-blue-500/10",
  testing: "text-cyan-300 border-cyan-500/40 bg-cyan-500/10",
  blocked: "text-red-300 border-red-500/40 bg-red-500/10",
  done: "text-emerald-300 border-emerald-500/40 bg-emerald-500/10",
};

function formatWhen(raw: string | null | undefined): string {
  if (!raw) return "—";
  const iso = /^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}/.test(raw)
    ? `${raw.replace(" ", "T")}${raw.endsWith("Z") || /[+-]\d{2}:\d{2}$/.test(raw) ? "" : "Z"}`
    : raw;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return raw;
  return d.toLocaleString();
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <div className="mb-0.5 text-[10px] uppercase tracking-wide text-[var(--text-faint)]">{label}</div>
      <div className="whitespace-pre-wrap text-[12px] leading-relaxed text-[var(--text)]">{children}</div>
    </div>
  );
}

export function GoalTaskModal({
  tasks,
  taskId,
  onClose,
}: {
  tasks: GoalTask[];
  taskId: string;
  onClose: () => void;
}) {
  const [viewId, setViewId] = useState(taskId);
  const [stack, setStack] = useState<string[]>([]);

  useEffect(() => {
    setViewId(taskId);
    setStack([]);
  }, [taskId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      if (stack.length > 0) {
        const prev = stack[stack.length - 1];
        setStack((s) => s.slice(0, -1));
        setViewId(prev);
      } else {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, stack]);

  const task = useMemo(() => tasks.find((t) => t.id === viewId) ?? null, [tasks, viewId]);
  const children = useMemo(
    () => (task ? goalTaskChildren(tasks, task.id) : []),
    [tasks, task],
  );
  const parent = useMemo(
    () => (task?.parent_id ? tasks.find((t) => t.id === task.parent_id) : null),
    [tasks, task],
  );

  useEffect(() => {
    if (!tasks.some((t) => t.id === viewId)) onClose();
  }, [tasks, viewId, onClose]);

  if (!task) return null;

  const openChild = (id: string) => {
    setStack((s) => [...s, viewId]);
    setViewId(id);
  };

  const goBack = () => {
    if (stack.length === 0) {
      onClose();
      return;
    }
    const prev = stack[stack.length - 1];
    setStack((s) => s.slice(0, -1));
    setViewId(prev);
  };

  const history = [...(task.history ?? [])].sort((a, b) => a.at.localeCompare(b.at));
  const colCls = COL_TONE[task.column] ?? COL_TONE.backlog;

  return createPortal(
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="flex max-h-[min(88vh,820px)] w-[min(640px,94vw)] flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface-2)] shadow-[var(--shadow-panel)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-start gap-2 border-b border-[var(--border)] px-4 py-3">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-1.5">
              {task.kind && (
                <span className={`text-[10px] ${KIND_COLOR[task.kind] ?? "text-gray-400"}`}>
                  {task.kind}
                </span>
              )}
              <span className={`rounded border px-1.5 py-0.5 text-[10px] ${colCls}`}>
                {task.column.replace(/_/g, " ")}
              </span>
              {parent && (
                <span className="truncate text-[10px] text-[var(--text-faint)]">
                  under {parent.title || parent.id}
                </span>
              )}
            </div>
            <h3 className="mt-1 text-[14px] font-medium text-[var(--text)]">{task.title || "Untitled task"}</h3>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {stack.length > 0 && (
              <button
                type="button"
                onClick={goBack}
                className="rounded-md border border-[var(--border)] px-2 py-1 text-[11px] text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
              >
                Back
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="rounded-md px-2 py-1 text-[12px] text-[var(--text-faint)] hover:bg-[var(--border)] hover:text-[var(--text)]"
            >
              ✕
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-3">
          {task.detail && <Field label="Detail">{task.detail}</Field>}

          {task.files && task.files.length > 0 && (
            <Field label="Files">
              <ul className="font-mono text-[11px] text-[var(--text-dim)]">
                {task.files.map((f) => (
                  <li key={f}>{f}</li>
                ))}
              </ul>
            </Field>
          )}

          {task.result && (
            <Field label="Result">
              <span className="text-emerald-300/90">{task.result}</span>
            </Field>
          )}
          {task.error && (
            <Field label="Error">
              <span className="text-red-300/90">{task.error}</span>
            </Field>
          )}

          <div className="grid grid-cols-2 gap-3 text-[11px] text-[var(--text-dim)]">
            <Field label="Created">{formatWhen(task.created_at)}</Field>
            <Field label="Updated">{formatWhen(task.updated_at)}</Field>
          </div>

          <div>
            <div className="mb-1.5 text-[10px] uppercase tracking-wide text-[var(--text-faint)]">
              Sub-tasks {children.length > 0 ? `(${children.filter((c) => c.column === "done").length}/${children.length})` : ""}
            </div>
            {children.length === 0 ? (
              <div className="text-[11px] text-[var(--text-faint)]">No sub-tasks yet.</div>
            ) : (
              <ul className="space-y-1">
                {children.map((child) => (
                  <li key={child.id}>
                    <button
                      type="button"
                      onClick={() => openChild(child.id)}
                      className="flex w-full items-center gap-2 rounded-md border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5 text-left hover:border-[var(--accent)]"
                    >
                      <span className={`rounded border px-1 py-0.5 text-[9px] ${COL_TONE[child.column] ?? COL_TONE.backlog}`}>
                        {child.column.replace(/_/g, " ")}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--text)]">
                        {child.title}
                      </span>
                      <span className="text-[10px] text-[var(--text-faint)]">details →</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <div>
            <div className="mb-1.5 text-[10px] uppercase tracking-wide text-[var(--text-faint)]">
              History
            </div>
            {history.length === 0 ? (
              <div className="text-[11px] text-[var(--text-faint)]">Nothing recorded on this card yet.</div>
            ) : (
              <ol className="space-y-2 border-l border-[var(--border)] pl-3">
                {history.map((ev, i) => (
                  <li key={`${ev.at}-${i}`} className="text-[12px]">
                    <div className="flex flex-wrap items-baseline gap-2">
                      <span className="font-mono text-[10px] text-[var(--text-faint)]">{formatWhen(ev.at)}</span>
                      <span className="text-[10px] uppercase tracking-wide text-violet-300">{ev.action}</span>
                      {ev.column && (
                        <span className="text-[10px] text-[var(--text-dim)]">{ev.column.replace(/_/g, " ")}</span>
                      )}
                    </div>
                    {ev.note && (
                      <div className="mt-0.5 whitespace-pre-wrap text-[11px] leading-relaxed text-[var(--text-dim)]">
                        {ev.note}
                      </div>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
