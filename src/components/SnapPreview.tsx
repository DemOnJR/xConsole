import { useSnapDragStore } from "../lib/snapDrag";
import { useCanvasStore } from "../stores/canvasStore";

/**
 * Highlights the swap / dock target under the cursor. One rectangle — not a
 * preset of zones that would reshuffle the whole grid.
 */
export function SnapPreview() {
  const nodeId = useSnapDragStore((s) => s.nodeId);
  const hint = useSnapDragStore((s) => s.hint);
  const pane = useCanvasStore((s) => s.paneSize);

  if (!nodeId || !hint || !pane || pane.width <= 0 || pane.height <= 0) return null;

  const label =
    hint.kind === "swap" ? "Swap" : hint.kind === "dock" ? `Dock ${hint.edge}` : `Place ${hint.edge}`;

  return (
    <div className="pointer-events-none absolute inset-0 z-40">
      <div
        className="absolute flex items-center justify-center rounded-md border border-blue-400/80 bg-blue-500/25 text-[11px] font-medium text-blue-100"
        style={{
          left: `${(hint.x / pane.width) * 100}%`,
          top: `${(hint.y / pane.height) * 100}%`,
          width: `${(hint.width / pane.width) * 100}%`,
          height: `${(hint.height / pane.height) * 100}%`,
        }}
      >
        {label}
      </div>
    </div>
  );
}
