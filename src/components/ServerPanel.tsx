import { useEffect, useMemo, useRef, useState } from "react";
import { useVpsStore } from "../stores/vpsStore";
import { startInternalDrag, useDragStore } from "../stores/dragStore";
import { useCanvasStore } from "../stores/canvasStore";
import type { Vps } from "../lib/tauri";
import { VpsForm } from "./VpsForm";
import { dialog } from "../stores/dialogStore";
import { PlusIcon, TrashIcon } from "./icons";
import { DrawerHeader } from "./DrawerHeader";
import { useMaskHost } from "../lib/privacy";
import { useQuickOpenStore } from "../stores/quickOpenStore";

export function ServerPanel({ width }: { width?: number }) {
  const { vpsList, load, remove, reorder } = useVpsStore();
  const maskHost = useMaskHost();
  const addVps = useCanvasStore((s) => s.addVps);
  const isDraggingRef = useRef(false);
  // Highlight comes from the shared drag state now that the drag is pointer-based.
  const dragOver = useDragStore((s) => s.over);

  const [query, setQuery] = useState("");
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<Vps | null>(null);
  const [pinned, setPinned] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("xconsole-pinned-servers") || "[]") as string[];
    } catch {
      return [];
    }
  });
  const togglePin = (id: string) => {
    setPinned((prev) => {
      const next = prev.includes(id) ? prev.filter((x) => x !== id) : [id, ...prev];
      try {
        localStorage.setItem("xconsole-pinned-servers", JSON.stringify(next.slice(0, 40)));
      } catch {
        /* ignore */
      }
      return next.slice(0, 40);
    });
  };

  useEffect(() => {
    load();
  }, [load]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const base = !q
      ? vpsList
      : vpsList.filter((v) =>
          [v.name, v.host, v.username, v.tags ?? ""]
            .join(" ")
            .toLowerCase()
            .includes(q),
        );
    // Pinned first, preserving relative order within each group.
    const pinSet = new Set(pinned);
    return [...base].sort((a, b) => {
      const ap = pinSet.has(a.id) ? 0 : 1;
      const bp = pinSet.has(b.id) ? 0 : 1;
      if (ap !== bp) return ap - bp;
      return pinned.indexOf(a.id) - pinned.indexOf(b.id);
    });
  }, [vpsList, query, pinned]);

  return (
    <aside
      className="xc-drawer flex h-full flex-col"
      data-side="right"
      style={{ width: width ?? "var(--drawer-w)" }}
    >
      <DrawerHeader
        title="Servers"
        actions={
          <>
            {pinned.length > 0 ? (
              <button
                type="button"
                className="rounded-md border border-[var(--border)] px-1.5 py-0.5 text-[10px] text-amber-200/90 hover:bg-[var(--border)]"
                data-tooltip="Open terminals for all pinned servers"
                onClick={() => {
                  for (const id of pinned) {
                    const v = vpsList.find((x) => x.id === id);
                    if (v) addVps(v);
                  }
                }}
              >
                ★ Open {pinned.length}
              </button>
            ) : null}
            <button
              className="flex items-center gap-1 rounded-md bg-blue-600 px-2 py-0.5 text-xs text-white hover:bg-blue-500"
              onClick={() => {
                setEditing(null);
                setShowForm(true);
              }}
            >
              <PlusIcon size={13} /> Add
            </button>
          </>
        }
      />

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
              isDraggingRef.current = false;
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
                () => {
                  isDraggingRef.current = true;
                },
              );
            }}
            className={`group mb-1 cursor-grab select-none rounded-md border px-2 py-2 hover:border-[var(--border)] hover:bg-[var(--surface)] active:cursor-grabbing ${
              dragOver === `server-row:${v.id}` ? "border-blue-500" : "border-transparent"
            }`}
            style={{ touchAction: "none" }}
            data-tooltip="Drag onto another server to reorder, or onto the canvas for an SSH terminal"
          >
            <div className="relative flex items-center gap-2">
              <span className="select-none text-gray-600">⋮⋮</span>
              <button
                className="min-w-0 flex-1 text-left"
                onClick={() => {
                  if (isDraggingRef.current) return;
                  addVps(v);
                }}
              >
                <div className="flex items-center gap-1 truncate text-sm text-gray-200">
                  {pinned.includes(v.id) ? (
                    <span className="shrink-0 text-[10px] text-amber-400" title="Pinned">
                      ★
                    </span>
                  ) : null}
                  <span className="truncate">{v.name}</span>
                </div>
                <div className="truncate text-xs text-gray-500">
                  {v.username}@{maskHost(v.host)}:{v.port}
                </div>
              </button>
              {/* Icons overlay the row on hover so they never steal text width.
                  Also visible on keyboard focus (group-focus-within) for a11y. */}
              <div className="absolute inset-y-0 right-0 flex items-center gap-1 bg-gradient-to-l from-[var(--surface)] via-[var(--surface)] to-transparent pl-3 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
                <button
                  className={`rounded p-1 text-xs hover:bg-[var(--border)] ${
                    pinned.includes(v.id) ? "text-amber-300 opacity-100" : "text-gray-500"
                  }`}
                  data-tooltip={pinned.includes(v.id) ? "Unpin" : "Pin to top"}
                  onClick={(e) => {
                    e.stopPropagation();
                    togglePin(v.id);
                  }}
                >
                  ★
                </button>
                <button
                  className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
                  data-tooltip={`Copy ${v.username}@${maskHost(v.host)}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    void navigator.clipboard.writeText(`${v.username}@${v.host}`);
                  }}
                >
                  @
                </button>
                <button
                  className="flex items-center gap-1 rounded bg-[var(--surface-2)] px-1.5 py-0.5 text-xs text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white border border-[var(--border)] transition shadow-sm"
                  data-tooltip="Fast Plugins & Actions (Ctrl+K)"
                  onClick={(e) => {
                    e.stopPropagation();
                    useQuickOpenStore.getState().open({ targetServer: v });
                  }}
                >
                  <span className="text-xs">⚡</span>
                  <span className="text-[11px] font-medium">Actions</span>
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
