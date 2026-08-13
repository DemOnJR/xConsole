import { useEffect, useMemo } from "react";
import { useEditsStore } from "../../stores/editsStore";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { lineDiff, type DiffResult } from "../../lib/diff";
import type { FileChange } from "../../lib/tauri";

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Right-docked drawer showing the files the agent edited — the live session's
 * changes, or persisted history filtered by workspace/session — with a
 * GitHub-style diff per file and one-click revert. */
export function ChangesPanel() {
  const open = useEditsStore((s) => s.open);
  const mode = useEditsStore((s) => s.mode);
  const setMode = useEditsStore((s) => s.setMode);
  const changes = useEditsStore((s) => s.changes);
  const historyGroups = useEditsStore((s) => s.historyGroups);
  const historyWorkspace = useEditsStore((s) => s.historyWorkspace);
  const historySession = useEditsStore((s) => s.historySession);
  const setHistoryFilters = useEditsStore((s) => s.setHistoryFilters);
  const loadHistory = useEditsStore((s) => s.loadHistory);
  const selectedId = useEditsStore((s) => s.selectedId);
  const reverting = useEditsStore((s) => s.reverting);
  const setOpen = useEditsStore((s) => s.setOpen);
  const select = useEditsStore((s) => s.select);
  const revert = useEditsStore((s) => s.revert);

  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const workspaceId = useWorkspaceStore((s) => s.activeId);

  // Load history whenever it opens or filters change.
  useEffect(() => {
    if (open && mode === "history") void loadHistory();
  }, [open, mode, historyWorkspace, historySession, loadHistory]);

  // Flat list of visible changes in the current mode.
  const visible: FileChange[] =
    mode === "live" ? changes : historyGroups.flatMap((g) => g.changes);

  const diffs = useMemo(() => {
    const map = new Map<string, DiffResult>();
    for (const c of visible) map.set(c.id, lineDiff(c.before, c.after));
    return map;
  }, [visible]);

  if (!open) return null;

  const selected = visible.find((c) => c.id === selectedId) ?? null;
  const selectedDiff = selected ? diffs.get(selected.id) ?? null : null;

  const filterInput = (id: string, label: string, value: string | null) => (
    <label className="flex flex-col gap-0.5 text-[10px] text-gray-500">
      {label}
      <select
        value={value ?? ""}
        onChange={(e) => {
          if (id === "workspace") setHistoryFilters(e.target.value || null, historySession);
          else setHistoryFilters(historyWorkspace, e.target.value || null);
        }}
        className="rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-1 text-[11px] text-gray-200 outline-none"
      >
        <option value="">All</option>
        {id === "workspace"
          ? workspaces.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))
          : null}
      </select>
    </label>
  );

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      <div
        className="absolute inset-0 bg-black/40"
        onClick={() => setOpen(false)}
        aria-hidden
      />
      <div className="relative flex h-full w-[min(960px,82vw)] flex-col border-l border-[var(--border)] bg-[var(--surface-2)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center gap-2 border-b border-[var(--border)] px-4 py-2.5">
          <span className="text-sm font-semibold text-gray-100">Agent Changes</span>
          <span className="rounded-full bg-[var(--border)] px-2 py-0.5 text-xs text-gray-300">
            {visible.length}
          </span>
          <div className="ml-2 flex overflow-hidden rounded border border-[var(--border)] text-[11px]">
            <button
              onClick={() => setMode("live")}
              className={`px-2 py-0.5 ${
                mode === "live"
                  ? "bg-[var(--accent-muted)] text-[var(--accent)]"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              This session
            </button>
            <button
              onClick={() => setMode("history")}
              className={`px-2 py-0.5 ${
                mode === "history"
                  ? "bg-[var(--accent-muted)] text-[var(--accent)]"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              History
            </button>
          </div>
          {mode === "history" && (
            <div className="ml-2 flex items-end gap-2">
              {filterInput("workspace", "Workspace", historyWorkspace)}
              {workspaceId ? (
                <button
                  onClick={() => setHistoryFilters(workspaceId, null)}
                  className="text-[10px] text-cyan-400 hover:underline"
                >
                  filter to current
                </button>
              ) : null}
            </div>
          )}
          <span className="ml-auto text-xs text-gray-500">
            {mode === "live"
              ? "files the agent edited this session"
              : "everything the agent changed, grouped by session"}
          </span>
          <button
            onClick={() => setOpen(false)}
            data-tooltip="Close"
            className="rounded-md px-2 py-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            ✕
          </button>
        </div>

        {visible.length === 0 ? (
          <div className="flex flex-1 items-center justify-center px-6 text-center">
            <div>
              <p className="text-sm text-gray-400">
                {mode === "live" ? "No edits yet." : "No history yet."}
              </p>
              <p className="mt-1 text-xs text-gray-600">
                When the agent writes a file (local or on a server), it shows up here with a
                diff and a one-click revert. History persists across restarts.
              </p>
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1">
            {/* File list */}
            <div className="w-80 shrink-0 overflow-y-auto border-r border-[var(--border)] py-1">
              {mode === "history" ? (
                historyGroups.map((g) => (
                  <div key={`${g.sessionId}::${g.workspaceId ?? ""}`} className="mb-1">
                    <div className="px-3 py-1 text-[10px] font-medium uppercase tracking-wide text-gray-500">
                      {g.workspaceId
                        ? workspaces.find((w) => w.id === g.workspaceId)?.name ?? g.workspaceId
                        : "Session"}{" "}
                      · {g.changes.length} file{g.changes.length > 1 ? "s" : ""}
                    </div>
                    {g.changes.map((c) => (
                      <ChangeRow
                        key={c.id}
                        c={c}
                        active={c.id === selectedId}
                        diff={diffs.get(c.id)}
                        onSelect={select}
                      />
                    ))}
                  </div>
                ))
              ) : (
                changes.map((c) => (
                  <ChangeRow
                    key={c.id}
                    c={c}
                    active={c.id === selectedId}
                    diff={diffs.get(c.id)}
                    onSelect={select}
                  />
                ))
              )}
            </div>

            {/* Diff view */}
            <div className="flex min-w-0 flex-1 flex-col">
              {selected ? (
                <>
                  <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-1.5">
                    <span
                      className="truncate font-mono text-xs text-gray-300"
                      data-tooltip={selected.path}
                    >
                      {selected.path}
                    </span>
                    <span className="shrink-0 rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-400">
                      {selected.scope === "local" ? "This PC" : selected.label}
                    </span>
                    <button
                      onClick={() => revert(selected.id)}
                      disabled={selected.reverted || reverting === selected.id}
                      data-tooltip={
                        selected.is_new
                          ? "Delete the file the agent created"
                          : "Restore the file's previous content"
                      }
                      className="ml-auto shrink-0 rounded-md border border-[var(--border)] px-2 py-1 text-xs text-gray-300 hover:bg-[var(--border)] disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      {selected.reverted
                        ? "Reverted"
                        : reverting === selected.id
                          ? "Reverting…"
                          : "Revert"}
                    </button>
                  </div>
                  <DiffBody diff={selectedDiff} />
                </>
              ) : (
                <div className="flex flex-1 items-center justify-center text-sm text-gray-500">
                  Select a file to see the diff.
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function ChangeRow({
  c,
  active,
  diff,
  onSelect,
}: {
  c: FileChange;
  active: boolean;
  diff?: DiffResult;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      onClick={() => onSelect(c.id)}
      className={`flex w-full flex-col gap-0.5 border-l-2 px-3 py-1.5 text-left ${
        active
          ? "border-blue-500 bg-[var(--surface)]"
          : "border-transparent hover:bg-[var(--surface)]"
      }`}
    >
      <div className="flex items-center gap-1.5">
        <span
          className={`truncate text-xs font-medium ${
            c.reverted ? "text-gray-500 line-through" : "text-gray-200"
          }`}
          data-tooltip={c.path}
        >
          {baseName(c.path)}
        </span>
        {c.is_new && (
          <span className="rounded bg-green-900/50 px-1 text-[9px] uppercase text-green-300">
            new
          </span>
        )}
        {c.reverted && (
          <span className="rounded bg-gray-700 px-1 text-[9px] uppercase text-gray-300">
            reverted
          </span>
        )}
      </div>
      <div className="flex items-center gap-2 text-[10px] text-gray-500">
        <span className="truncate">{c.label}</span>
        {diff && (
          <span className="ml-auto shrink-0 font-mono">
            <span className="text-green-400">+{diff.added}</span>{" "}
            <span className="text-red-400">−{diff.removed}</span>
          </span>
        )}
      </div>
    </button>
  );
}

function DiffBody({ diff }: { diff: DiffResult | null }) {
  if (!diff) return null;
  if (diff.rows.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-gray-500">
        (empty file)
      </div>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-auto bg-[var(--bg)]">
      <table className="w-full border-collapse font-mono text-[11.5px] leading-[1.5]">
        <tbody>
          {diff.rows.map((r, i) => {
            const bg =
              r.type === "add"
                ? "bg-green-500/10"
                : r.type === "del"
                  ? "bg-red-500/10"
                  : "";
            const sign = r.type === "add" ? "+" : r.type === "del" ? "−" : " ";
            const signColor =
              r.type === "add"
                ? "text-green-400"
                : r.type === "del"
                  ? "text-red-400"
                  : "text-gray-600";
            return (
              <tr key={i} className={bg}>
                <td className="select-none border-r border-[var(--border)] px-2 text-right align-top text-[10px] text-gray-600">
                  {r.oldNo ?? ""}
                </td>
                <td className="select-none border-r border-[var(--border)] px-2 text-right align-top text-[10px] text-gray-600">
                  {r.newNo ?? ""}
                </td>
                <td className={`select-none px-1 text-center align-top ${signColor}`}>
                  {sign}
                </td>
                <td className="whitespace-pre-wrap break-all px-2 align-top text-gray-200">
                  {r.text || " "}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
