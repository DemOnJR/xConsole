import { useDragStore } from "../stores/dragStore";

/**
 * The thing that follows the cursor during an internal drag.
 *
 * HTML5 DnD drew this for free; with pointer-event drags we draw it ourselves. Rendered
 * once at app level and positioned fixed, so it is never clipped by a node's overflow and
 * never inherits the canvas transform.
 */
export function DragGhost() {
  const drag = useDragStore((s) => s.drag);
  const x = useDragStore((s) => s.x);
  const y = useDragStore((s) => s.y);
  if (!drag) return null;

  return (
    <div
      className="pointer-events-none fixed z-[9999] flex max-w-[280px] items-center gap-1.5 truncate rounded-md border border-[var(--accent)] bg-[var(--surface)] px-2 py-1 text-xs text-gray-100 shadow-xl"
      style={{ left: x + 12, top: y + 12 }}
    >
      <span className="text-[var(--accent)]">
        {drag.kind === "vps"
          ? "▤"
          : drag.kind === "local-file"
            ? drag.isDir
              ? "📁"
              : "⬆"
            : drag.isDir
              ? "▸"
              : "○"}
      </span>
      <span className="truncate">{drag.label}</span>
      {drag.paths && drag.paths.length > 1 ? (
        <span className="shrink-0 rounded bg-[var(--accent)]/20 px-1 text-[10px] text-[var(--accent)]">
          ×{drag.paths.length}
        </span>
      ) : null}
    </div>
  );
}
