import { useEffect, useMemo, useState } from "react";
import { useVpsStore } from "../stores/vpsStore";
import { startInternalDrag, useDragStore } from "../stores/dragStore";
import { useCanvasStore } from "../stores/canvasStore";
import type { Vps } from "../lib/tauri";
import { VpsForm } from "./VpsForm";
import { dialog } from "../stores/dialogStore";
import { PlusIcon, TrashIcon, FolderIcon, DatabaseIcon } from "./icons";

export function ServerPanel() {
  const { vpsList, load, remove, reorder } = useVpsStore();
  const addVps = useCanvasStore((s) => s.addVps);
  const addSftp = useCanvasStore((s) => s.addSftp);
  const addDb = useCanvasStore((s) => s.addDb);
  // Highlight comes from the shared drag state now that the drag is pointer-based.
  const dragOver = useDragStore((s) => s.over);

  const [query, setQuery] = useState("");
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<Vps | null>(null);

  useEffect(() => {
    load();
  }, [load]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return vpsList;
    return vpsList.filter((v) =>
      [v.name, v.host, v.username, v.tags ?? ""]
        .join(" ")
        .toLowerCase()
        .includes(q),
    );
  }, [vpsList, query]);

  return (
    <aside className="xc-drawer flex h-full flex-col" data-side="right" style={{ width: "var(--drawer-w)" }}>
      <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-2.5">
        <span className="xc-panel-title">Servers</span>
        <div className="ml-auto flex items-center gap-1">
          <button
            className="flex items-center gap-1 rounded-md bg-blue-600 px-2 py-0.5 text-xs text-white hover:bg-blue-500"
            onClick={() => {
              setEditing(null);
              setShowForm(true);
            }}
          >
            <PlusIcon size={13} /> Add
          </button>
        </div>
      </div>

      <div className="px-3 pb-2 pt-2">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search servers..."
          className="w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-xs text-gray-200 outline-none focus:border-blue-500"
        />
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2">
        {filtered.length === 0 && (
          <p className="px-2 py-6 text-center text-xs text-gray-600">
            No servers yet. Click “Add”.
          </p>
        )}
        {filtered.map((v) => (
          <div
            key={v.id}
            // Pointer-event drag, not HTML5: the webview stops delivering drag events
            // once Tauri intercepts native drags so the app can receive dropped files.
            data-drop={`server-row:${v.id}`}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              startInternalDrag(
                e,
                { kind: "vps", vpsId: v.id, label: v.name },
                (target, payload) => {
                  const row = target.startsWith("server-row:")
                    ? target.slice("server-row:".length)
                    : null;
                  // Dropped on another server → reorder. Dropped on the canvas →
                  // CanvasFlow's own target handles adding the terminal.
                  if (row && row !== payload.vpsId) void reorder(payload.vpsId, row);
                },
              );
            }}
            className={`group mb-1 cursor-grab rounded-md border px-2 py-2 hover:border-[var(--border)] hover:bg-[var(--surface)] active:cursor-grabbing ${
              dragOver === `server-row:${v.id}` ? "border-blue-500" : "border-transparent"
            }`}
            data-tooltip="Drag onto another server to reorder, or onto the canvas for an SSH terminal"
          >
            <div className="flex items-center gap-2">
              <span className="select-none text-gray-600">⋮⋮</span>
              <button
                className="min-w-0 flex-1 text-left"
                onClick={() => addVps(v)}
              >
                <div className="truncate text-sm text-gray-200">{v.name}</div>
                <div className="truncate text-xs text-gray-500">
                  {v.username}@{v.host}:{v.port}
                </div>
              </button>
              <div className="flex items-center gap-1 opacity-0 transition group-hover:opacity-100">
                <button
                  className="rounded p-1 text-cyan-400/80 hover:bg-[var(--border)] hover:text-cyan-300"
                  data-tooltip="Open SFTP on canvas"
                  onClick={(e) => {
                    e.stopPropagation();
                    addSftp(v);
                  }}
                >
                  <FolderIcon size={14} />
                </button>
                <button
                  className="rounded p-1 text-violet-400/80 hover:bg-[var(--border)] hover:text-violet-300"
                  data-tooltip="Open databases on canvas"
                  onClick={(e) => {
                    e.stopPropagation();
                    addDb(v);
                  }}
                >
                  <DatabaseIcon size={14} />
                </button>
                <button
                  className="rounded px-1 text-xs text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
                  data-tooltip="Edit"
                  onClick={() => {
                    setEditing(v);
                    setShowForm(true);
                  }}
                >
                  ✎
                </button>
                <button
                  className="rounded p-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-red-300"
                  data-tooltip="Delete"
                  onClick={async () => {
                    if (
                      await dialog.confirm({
                        title: "Delete server",
                        message: `Delete ${v.name}?`,
                        danger: true,
                        confirmText: "Delete",
                      })
                    )
                      remove(v.id);
                  }}
                >
                  <TrashIcon size={14} />
                </button>
              </div>
            </div>
            {v.tags && (
              <div className="mt-1 flex flex-wrap gap-1 pl-5">
                {v.tags
                  .split(",")
                  .map((t) => t.trim())
                  .filter(Boolean)
                  .map((t, i) => (
                    <span
                      key={`${t}-${i}`}
                      className="rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-400"
                    >
                      {t}
                    </span>
                  ))}
              </div>
            )}
          </div>
        ))}
      </div>

      {showForm && (
        <VpsForm initial={editing} onClose={() => setShowForm(false)} />
      )}
    </aside>
  );
}
