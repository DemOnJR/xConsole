import { useMemo } from "react";
import { useEditsStore } from "../../stores/editsStore";
import { lineDiff, type DiffResult } from "../../lib/diff";

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Right-docked drawer showing the files the agent edited this session, with a
 * GitHub-style diff per file and one-click revert. */
export function ChangesPanel() {
  const open = useEditsStore((s) => s.open);
  const changes = useEditsStore((s) => s.changes);
  const selectedId = useEditsStore((s) => s.selectedId);
  const reverting = useEditsStore((s) => s.reverting);
  const setOpen = useEditsStore((s) => s.setOpen);
  const select = useEditsStore((s) => s.select);
  const revert = useEditsStore((s) => s.revert);

  // Diff (and +/- counts) per change, memoized so we don't recompute on every render.
  const diffs = useMemo(() => {
    const map = new Map<string, DiffResult>();
    for (const c of changes) map.set(c.id, lineDiff(c.before, c.after));
    return map;
  }, [changes]);

  if (!open) return null;

  const selected = changes.find((c) => c.id === selectedId) ?? null;
  const selectedDiff = selected ? diffs.get(selected.id) ?? null : null;

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
          <span className="text-sm font-semibold text-gray-100">Changes</span>
          <span className="rounded-full bg-[var(--border)] px-2 py-0.5 text-xs text-gray-300">
            {changes.length}
          </span>
          <span className="text-xs text-gray-500">files the agent edited this session</span>
          <button
            onClick={() => setOpen(false)}
            data-tooltip="Close"
            className="ml-auto rounded-md px-2 py-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            ✕
          </button>
        </div>

        {changes.length === 0 ? (
          <div className="flex flex-1 items-center justify-center px-6 text-center">
            <div>
              <p className="text-sm text-gray-400">No edits yet.</p>
              <p className="mt-1 text-xs text-gray-600">
                When the agent writes a file (local or on a server), it shows up here with a
                diff and a one-click revert.
              </p>
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1">
            {/* File list */}
            <div className="w-72 shrink-0 overflow-y-auto border-r border-[var(--border)] py-1">
              {changes.map((c) => {
                const d = diffs.get(c.id);
                const active = c.id === selectedId;
                return (
                  <button
                    key={c.id}
                    onClick={() => select(c.id)}
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
                      {d && (
                        <span className="ml-auto shrink-0 font-mono">
                          <span className="text-green-400">+{d.added}</span>{" "}
                          <span className="text-red-400">−{d.removed}</span>
                        </span>
                      )}
                    </div>
                  </button>
                );
              })}
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
