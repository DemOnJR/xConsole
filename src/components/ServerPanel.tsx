import { useEffect, useMemo, useState } from "react";
import { useVpsStore } from "../stores/vpsStore";
import { useCanvasStore } from "../stores/canvasStore";
import type { Vps } from "../lib/tauri";
import { VpsForm } from "./VpsForm";
import { dialog } from "../stores/dialogStore";
import { PlusIcon, TrashIcon, FolderIcon } from "./icons";

export const VPS_DND_MIME = "application/x-vps-id";

export function ServerPanel() {
  const { vpsList, load, remove, reorder } = useVpsStore();
  const addVps = useCanvasStore((s) => s.addVps);
  const addSftp = useCanvasStore((s) => s.addSftp);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);

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
    <aside className="flex h-full w-72 shrink-0 flex-col border-l border-[var(--border)] bg-[var(--surface-2)]">
      <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-2.5">
        <span className="text-xs font-medium uppercase tracking-wider text-gray-400">
          Servers
        </span>
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
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(VPS_DND_MIME, v.id);
              // copy → drop on canvas adds a node; move → drop on a row reorders.
              e.dataTransfer.effectAllowed = "copyMove";
              setDragId(v.id);
            }}
            onDragEnd={() => {
              setDragId(null);
              setDropTargetId(null);
            }}
            onDragOver={(e) => {
              // Only a list-internal drag reorders; a drag from elsewhere is ignored here.
              if (!dragId || dragId === v.id) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              if (dropTargetId !== v.id) setDropTargetId(v.id);
            }}
            onDragLeave={() => {
              if (dropTargetId === v.id) setDropTargetId(null);
            }}
            onDrop={(e) => {
              const src = e.dataTransfer.getData(VPS_DND_MIME);
              if (!src || src === v.id) return;
              e.preventDefault();
              e.stopPropagation(); // don't let the canvas treat this as an add
              void reorder(src, v.id);
              setDragId(null);
              setDropTargetId(null);
            }}
            className={`group mb-1 cursor-grab rounded-md border px-2 py-2 hover:border-[var(--border)] hover:bg-[var(--surface)] active:cursor-grabbing ${
              dropTargetId === v.id ? "border-blue-500" : "border-transparent"
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
