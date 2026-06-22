import { useEffect, useState } from "react";
import {
  useWorkspaceStore,
  WORKSPACE_COLORS,
} from "../stores/workspaceStore";
import type { ColorMode, Workspace } from "../lib/tauri";
import {
  accentStyle,
  COLOR_MODES,
  DEFAULT_COLOR,
  DEFAULT_COLOR_MODE,
  DEFAULT_ICON,
} from "../lib/workspaceStyle";
import { useOpenWorkspace } from "../hooks/useOpenWorkspace";
import { EmojiPicker } from "./EmojiPicker";
import { PaletteIcon, TrashIcon } from "./icons";

function WorkspaceRow({ w }: { w: Workspace }) {
  const activeId = useWorkspaceStore((s) => s.activeId);
  const removeWorkspace = useWorkspaceStore((s) => s.remove);
  const updateMeta = useWorkspaceStore((s) => s.updateMeta);
  const openWorkspace = useOpenWorkspace();

  const [picker, setPicker] = useState(false);
  const color = w.color || DEFAULT_COLOR;
  const icon = w.icon || DEFAULT_ICON;
  const mode = (w.color_mode as ColorMode) || DEFAULT_COLOR_MODE;
  const isActive = activeId === w.id;

  return (
    <div className="relative">
      <div
        className="group mb-1 flex items-center gap-2 rounded-md border border-transparent px-2 py-1.5 hover:bg-[#11161f]"
        style={accentStyle(color, mode, isActive)}
      >
        <span className="text-base leading-none">{icon}</span>
        <button
          className="min-w-0 flex-1 truncate text-left text-sm text-gray-200"
          onClick={() => openWorkspace(w.id)}
          title="Open workspace"
        >
          {w.name}
        </button>

        <button
          className="rounded p-0.5 opacity-0 transition group-hover:opacity-100"
          title="Color & icon"
          onClick={() => setPicker((p) => !p)}
          style={{ color }}
        >
          <PaletteIcon size={15} />
        </button>
        <button
          className="rounded p-0.5 text-gray-500 opacity-0 transition hover:text-red-300 group-hover:opacity-100"
          title="Delete workspace"
          onClick={() => removeWorkspace(w.id)}
        >
          <TrashIcon size={15} />
        </button>
      </div>

      {picker && (
        <div
          className="absolute left-1 right-1 z-30 mt-1 rounded-lg border border-[#1f2737] bg-[#11161f] p-2.5 shadow-2xl"
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
              className="relative h-5 w-5 cursor-pointer overflow-hidden rounded-full ring-1 ring-[#2a3346]"
              title="Custom color"
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
          <div className="mb-2 flex overflow-hidden rounded-md border border-[#1f2737]">
            {COLOR_MODES.map((m) => (
              <button
                key={m.id}
                onClick={() => updateMeta(w.id, { colorMode: m.id })}
                className={`flex-1 px-2 py-1 text-[11px] ${
                  mode === m.id
                    ? "bg-blue-600 text-white"
                    : "text-gray-300 hover:bg-[#1f2737]"
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
    </div>
  );
}

export function WorkspacePanel() {
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const loadWorkspaces = useWorkspaceStore((s) => s.load);

  useEffect(() => {
    loadWorkspaces();
  }, [loadWorkspaces]);

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-[#1f2737] bg-[#0d121b]">
      <div className="flex items-center gap-2 border-b border-[#1f2737] px-3 py-2.5">
        <div className="text-sm font-semibold tracking-wide text-gray-100">
          Workspaces
        </div>
      </div>

      <div className="px-3 pb-1 pt-3">
        <span className="text-xs font-medium uppercase tracking-wider text-gray-500">
          Workspaces
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 py-1">
        {workspaces.length === 0 && (
          <p className="px-2 py-6 text-center text-xs text-gray-600">
            No saved workspaces. Drop servers on the canvas, then click Save in the
            top toolbar.
          </p>
        )}
        {workspaces.map((w) => (
          <WorkspaceRow key={w.id} w={w} />
        ))}
      </div>
    </aside>
  );
}
