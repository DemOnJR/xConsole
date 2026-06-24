import { useEffect, useState } from "react";
import {
  useWorkspaceStore,
  WORKSPACE_COLORS,
} from "../stores/workspaceStore";
import { useVpsStore } from "../stores/vpsStore";
import { useCanvasStore } from "../stores/canvasStore";
import { dialog } from "../stores/dialogStore";
import { useAgentStatusStore, STATUS_META } from "../stores/agentStatusStore";
import { api } from "../lib/tauri";
import type { ColorMode, Workspace, WorkspaceProject } from "../lib/tauri";
import {
  accentStyle,
  COLOR_MODES,
  DEFAULT_COLOR,
  DEFAULT_COLOR_MODE,
  DEFAULT_ICON,
} from "../lib/workspaceStyle";
import { useOpenWorkspace } from "../hooks/useOpenWorkspace";
import { EmojiPicker } from "./EmojiPicker";
import { FolderIcon, PaletteIcon, PlusIcon, TrashIcon } from "./icons";

/** Project location + brief editor for a workspace, shown in the settings popover. */
function ProjectSettings({ w }: { w: Workspace }) {
  const setProject = useWorkspaceStore((s) => s.setProject);
  const vpsList = useVpsStore((s) => s.vpsList);

  const initial: WorkspaceProject | null = (() => {
    try {
      return w.project_json ? (JSON.parse(w.project_json) as WorkspaceProject) : null;
    } catch {
      return null;
    }
  })();

  const [kind, setKind] = useState<"local" | "vps">(initial?.kind ?? "local");
  const [path, setPath] = useState(initial?.path ?? "");
  const [vpsId, setVpsId] = useState(initial?.vps_id ?? "");
  const [brief, setBrief] = useState("");
  const [msg, setMsg] = useState("");

  useEffect(() => {
    let alive = true;
    api
      .getWorkspaceBrief(w.id)
      .then((b) => alive && setBrief(b))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [w.id]);

  const flash = (m: string) => {
    setMsg(m);
    setTimeout(() => setMsg(""), 1500);
  };

  const saveProject = async () => {
    if (!path.trim()) {
      await setProject(w.id, null);
    } else {
      await setProject(w.id, {
        kind,
        path: path.trim(),
        ...(kind === "vps" && vpsId ? { vps_id: vpsId } : {}),
      });
    }
    flash("Project saved");
  };

  const saveBrief = async () => {
    await api.saveWorkspaceBrief(w.id, brief);
    flash("Brief saved");
  };

  const inputCls =
    "w-full rounded border border-[var(--border-strong)] bg-[var(--bg)] px-2.5 py-1.5 text-sm text-gray-200 outline-none placeholder:text-gray-600 focus:border-[#3d4a61]";

  return (
    <div>
      <div className="mb-1.5 text-xs uppercase tracking-wider text-gray-500">
        Project (gives the agent context)
      </div>
      <div className="mb-1.5 flex overflow-hidden rounded-md border border-[var(--border)]">
        {(["local", "vps"] as const).map((k) => (
          <button
            key={k}
            onClick={() => setKind(k)}
            className={`flex-1 px-2 py-1.5 text-sm ${
              kind === k ? "bg-blue-600 text-white" : "text-gray-300 hover:bg-[var(--border)]"
            }`}
          >
            {k === "local" ? "Local folder" : "VPS path"}
          </button>
        ))}
      </div>
      {kind === "vps" && (
        <select
          value={vpsId}
          onChange={(e) => setVpsId(e.target.value)}
          className={`${inputCls} mb-1.5`}
        >
          <option value="">Select a server…</option>
          {vpsList.map((v) => (
            <option key={v.id} value={v.id}>
              {v.name}
            </option>
          ))}
        </select>
      )}
      <input
        value={path}
        onChange={(e) => setPath(e.target.value)}
        placeholder={kind === "local" ? "C:\\dev\\myproject" : "/var/www/app"}
        className={`${inputCls} mb-1.5`}
      />
      <div className="mb-3 flex items-center justify-between">
        <span className="text-xs text-emerald-400">{msg}</span>
        <button
          onClick={saveProject}
          className="rounded bg-[var(--border)] px-3 py-1.5 text-sm text-gray-200 hover:bg-[#28324a]"
        >
          Save project
        </button>
      </div>

      <div className="mb-1.5 text-xs uppercase tracking-wider text-gray-500">
        Project brief (the agent keeps this updated)
      </div>
      <textarea
        value={brief}
        onChange={(e) => setBrief(e.target.value)}
        rows={16}
        placeholder="What is this project? Where do things live? Conventions… (the agent fills this in on the first task, and you can edit it.)"
        className={`${inputCls} mb-2 min-h-[260px] resize-y font-mono leading-relaxed`}
      />
      <div className="flex justify-end">
        <button
          onClick={saveBrief}
          className="rounded bg-[var(--border)] px-3 py-1.5 text-sm text-gray-200 hover:bg-[#28324a]"
        >
          Save brief
        </button>
      </div>
    </div>
  );
}

function WorkspaceRow({ w }: { w: Workspace }) {
  const activeId = useWorkspaceStore((s) => s.activeId);
  const removeWorkspace = useWorkspaceStore((s) => s.remove);
  const updateMeta = useWorkspaceStore((s) => s.updateMeta);
  const deselect = useWorkspaceStore((s) => s.deselect);
  const setNodes = useCanvasStore((s) => s.setNodes);
  const setEdges = useCanvasStore((s) => s.setEdges);
  const openWorkspace = useOpenWorkspace();

  const status = useAgentStatusStore((s) => s.byWorkspace[w.id]);
  const [picker, setPicker] = useState(false);
  const [projectOpen, setProjectOpen] = useState(false);
  const color = w.color || DEFAULT_COLOR;
  const icon = w.icon || DEFAULT_ICON;
  const mode = (w.color_mode as ColorMode) || DEFAULT_COLOR_MODE;
  const isActive = activeId === w.id;

  useEffect(() => {
    if (!projectOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setProjectOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [projectOpen]);

  return (
    <div className="relative">
      <div
        className="group mb-1 flex items-center gap-2 rounded-md border border-transparent px-2 py-1.5 hover:bg-[var(--surface)]"
        style={accentStyle(color, mode, isActive)}
      >
        <span className="relative text-base leading-none">
          {icon}
          {status && (
            <span
              className="absolute -right-1 -top-1 h-2 w-2 animate-pulse rounded-full"
              style={{ background: STATUS_META[status].color }}
            />
          )}
        </span>
        <button
          className="min-w-0 flex-1 truncate text-left text-sm text-gray-200"
          onClick={() => {
            if (isActive) {
              // Toggle off: close the workspace (clear the canvas, no selection).
              setNodes([]);
              setEdges([]);
              deselect();
            } else {
              openWorkspace(w.id);
            }
          }}
          data-tooltip={
            status
              ? `${w.name} — ${STATUS_META[status].label}`
              : isActive
                ? "Close workspace (click to unselect)"
                : "Open workspace"
          }
        >
          {w.name}
          {status && (
            <span className="ml-1.5 text-[10px] text-[var(--text-faint)]">
              {STATUS_META[status].label}
            </span>
          )}
        </button>

        <button
          className="rounded p-0.5 opacity-0 transition group-hover:opacity-100"
          data-tooltip="Color & icon"
          onClick={() => {
            setPicker((p) => !p);
            setProjectOpen(false);
          }}
          style={{ color }}
        >
          <PaletteIcon size={15} />
        </button>
        <button
          className="rounded p-0.5 text-gray-400 opacity-0 transition hover:text-gray-200 group-hover:opacity-100"
          data-tooltip="Project & agent context"
          onClick={() => {
            setProjectOpen((p) => !p);
            setPicker(false);
          }}
        >
          <FolderIcon size={15} />
        </button>
        <button
          className="rounded p-0.5 text-gray-500 opacity-0 transition hover:text-red-300 group-hover:opacity-100"
          data-tooltip="Delete workspace"
          onClick={async () => {
            if (
              await dialog.confirm({
                title: "Delete workspace",
                message: `Delete workspace "${w.name}"?`,
                danger: true,
                confirmText: "Delete",
              })
            )
              void removeWorkspace(w.id);
          }}
        >
          <TrashIcon size={15} />
        </button>
      </div>

      {picker && (
        <div
          className="absolute left-1 right-1 z-30 mt-1 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-2.5 shadow-2xl"
          onMouseLeave={() => setPicker(false)}
        >
          {/* Color: presets + native picker */}
          <div className="mb-1 text-[10px] uppercase tracking-wider text-gray-500">
            Color
          </div>
          <div className="mb-2 flex flex-wrap items-center gap-1.5">
            {WORKSPACE_COLORS.map((c) => (
              <button
                key={c}
                className={`h-5 w-5 rounded-full ${
                  color.toLowerCase() === c.toLowerCase()
                    ? "ring-2 ring-white"
                    : ""
                }`}
                style={{ background: c }}
                onClick={() => updateMeta(w.id, { color: c })}
              />
            ))}
            <label
              className="relative h-5 w-5 cursor-pointer overflow-hidden rounded-full ring-1 ring-[var(--border-strong)]"
              data-tooltip="Custom color"
              style={{
                background:
                  "conic-gradient(red, yellow, lime, aqua, blue, magenta, red)",
              }}
            >
              <input
                type="color"
                value={color}
                onChange={(e) => updateMeta(w.id, { color: e.target.value })}
                className="absolute inset-0 cursor-pointer opacity-0"
              />
            </label>
            <span className="ml-1 font-mono text-[11px] text-gray-500">
              {color}
            </span>
          </div>

          {/* Where the color goes */}
          <div className="mb-1 text-[10px] uppercase tracking-wider text-gray-500">
            Apply color to
          </div>
          <div className="mb-2 flex overflow-hidden rounded-md border border-[var(--border)]">
            {COLOR_MODES.map((m) => (
              <button
                key={m.id}
                onClick={() => updateMeta(w.id, { colorMode: m.id })}
                className={`flex-1 px-2 py-1 text-[11px] ${
                  mode === m.id
                    ? "bg-blue-600 text-white"
                    : "text-gray-300 hover:bg-[var(--border)]"
                }`}
              >
                {m.label}
              </button>
            ))}
          </div>

          {/* Icon search */}
          <div className="mb-1 text-[10px] uppercase tracking-wider text-gray-500">
            Icon
          </div>
          <EmojiPicker
            value={icon}
            onPick={(emoji) => updateMeta(w.id, { icon: emoji })}
          />
        </div>
      )}

      {projectOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
          onClick={() => setProjectOpen(false)}
        >
          <div
            className="flex max-h-[85vh] w-[min(760px,92vw)] flex-col overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface-2)] shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center gap-2 border-b border-[var(--border)] px-4 py-2.5">
              <span className="text-base leading-none">{icon}</span>
              <span className="text-sm font-semibold text-gray-100">Project &amp; Agent Context</span>
              <span className="truncate text-xs text-gray-500">{w.name}</span>
              <button
                onClick={() => setProjectOpen(false)}
                data-tooltip="Close"
                className="ml-auto rounded-md px-2 py-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
              >
                ✕
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-4">
              <ProjectSettings w={w} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Compact rail item shown when the sidebar is collapsed: icon + accent only. */
function WorkspaceRailItem({ w }: { w: Workspace }) {
  const activeId = useWorkspaceStore((s) => s.activeId);
  const deselect = useWorkspaceStore((s) => s.deselect);
  const setNodes = useCanvasStore((s) => s.setNodes);
  const setEdges = useCanvasStore((s) => s.setEdges);
  const status = useAgentStatusStore((s) => s.byWorkspace[w.id]);
  const openWorkspace = useOpenWorkspace();
  const isActive = activeId === w.id;
  const color = w.color || DEFAULT_COLOR;
  const icon = w.icon || DEFAULT_ICON;
  return (
    <button
      data-tooltip={status ? `${w.name} — ${STATUS_META[status].label}` : w.name}
      data-tooltip-side="right"
      onClick={() => {
        if (isActive) {
          setNodes([]);
          setEdges([]);
          deselect();
        } else {
          openWorkspace(w.id);
        }
      }}
      className="relative flex h-9 w-9 items-center justify-center rounded-md border text-base transition hover:bg-[var(--surface)]"
      style={
        isActive
          ? { borderColor: color, boxShadow: `0 0 0 1px ${color}` }
          : { borderColor: "transparent" }
      }
    >
      {icon}
      {status && (
        <span
          className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 animate-pulse rounded-full ring-2 ring-[var(--surface-2)]"
          style={{ background: STATUS_META[status].color }}
        />
      )}
    </button>
  );
}

const WS_COLLAPSE_KEY = "ui.workspaces.collapsed";

export function WorkspacePanel() {
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const loadWorkspaces = useWorkspaceStore((s) => s.load);
  const createNew = useWorkspaceStore((s) => s.createNew);
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(WS_COLLAPSE_KEY) === "1",
  );

  useEffect(() => {
    loadWorkspaces();
  }, [loadWorkspaces]);

  const toggleCollapsed = () =>
    setCollapsed((c) => {
      const next = !c;
      localStorage.setItem(WS_COLLAPSE_KEY, next ? "1" : "0");
      return next;
    });

  const newWorkspace = async () => {
    const name = await dialog.prompt({
      title: "New workspace",
      label: "Name",
      placeholder: "My project",
      confirmText: "Create",
    });
    if (name && name.trim()) await createNew(name.trim());
  };

  return (
    <aside
      className={`flex h-full shrink-0 flex-col border-r border-[var(--border)] bg-[var(--surface-2)] ${
        collapsed ? "w-14" : "w-64"
      }`}
    >
      <div className="flex items-center gap-1 border-b border-[var(--border)] px-2 py-2.5">
        {!collapsed && (
          <div className="ml-1 flex-1 text-sm font-semibold tracking-wide text-gray-100">
            Workspaces
          </div>
        )}
        {!collapsed && (
          <button
            onClick={newWorkspace}
            data-tooltip="New workspace"
            className="rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            <PlusIcon size={15} />
          </button>
        )}
        <button
          onClick={toggleCollapsed}
          data-tooltip={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          data-tooltip-side={collapsed ? "right" : undefined}
          className={`rounded-md p-1 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 ${
            collapsed ? "mx-auto" : ""
          }`}
        >
          {collapsed ? "»" : "«"}
        </button>
      </div>

      {collapsed ? (
        <div className="flex min-h-0 flex-1 flex-col items-center gap-1.5 overflow-y-auto px-1 py-2">
          <button
            onClick={newWorkspace}
            data-tooltip="New workspace"
            data-tooltip-side="right"
            className="flex h-9 w-9 items-center justify-center rounded-md border border-dashed border-[var(--border-strong)] text-gray-400 hover:bg-[var(--surface)] hover:text-gray-200"
          >
            <PlusIcon size={15} />
          </button>
          {workspaces.map((w) => (
            <WorkspaceRailItem key={w.id} w={w} />
          ))}
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
          {workspaces.length === 0 && (
            <p className="px-2 py-6 text-center text-xs text-gray-600">
              No workspaces yet. Click + to create one, or drop servers on the canvas
              and Save from the top toolbar.
            </p>
          )}
          {workspaces.map((w) => (
            <WorkspaceRow key={w.id} w={w} />
          ))}
        </div>
      )}
    </aside>
  );
}
