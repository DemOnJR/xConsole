import {
  BaseEdge,
  EdgeLabelRenderer,
  getStraightPath,
  useInternalNode,
  useReactFlow,
  type EdgeProps,
} from "@xyflow/react";

// A "floating" edge: instead of attaching to fixed handle positions (which makes
// the SSH↔SFTP link loop around when the panels move), it connects the two
// nodes border-to-border on whichever sides face each other, so the line tracks
// the windows wherever they are on the canvas.

type XY = { x: number; y: number };

/** Point on `node`'s border, on the line toward `other`'s center. */
function borderPoint(node: ReturnType<typeof useInternalNode>, other: ReturnType<typeof useInternalNode>): XY {
  if (!node || !other) return { x: 0, y: 0 };
  const w = (node.measured?.width ?? 0) / 2;
  const h = (node.measured?.height ?? 0) / 2;
  const cx = node.internals.positionAbsolute.x + w;
  const cy = node.internals.positionAbsolute.y + h;
  const ox = other.internals.positionAbsolute.x + (other.measured?.width ?? 0) / 2;
  const oy = other.internals.positionAbsolute.y + (other.measured?.height ?? 0) / 2;

  const dx = ox - cx;
  const dy = oy - cy;
  if (dx === 0 && dy === 0) return { x: cx, y: cy };
  // Scale the direction vector to the node's rectangle border.
  const scale = 1 / Math.max(Math.abs(dx) / (w || 1), Math.abs(dy) / (h || 1));
  return { x: cx + dx * scale, y: cy + dy * scale };
}

export function FloatingEdge({ id, source, target, markerEnd, style }: EdgeProps) {
  const sourceNode = useInternalNode(source);
  const targetNode = useInternalNode(target);
  const { deleteElements } = useReactFlow();
  if (!sourceNode || !targetNode) return null;

  const s = borderPoint(sourceNode, targetNode);
  const t = borderPoint(targetNode, sourceNode);
  const [path] = getStraightPath({
    sourceX: s.x,
    sourceY: s.y,
    targetX: t.x,
    targetY: t.y,
  });

  // Both endpoint dots are drawn here (not as fixed node handles) so they ride
  // the line's border-to-border endpoints and move with the windows.
  const color = (style?.stroke as string) ?? "#22d3ee";
  const mx = (s.x + t.x) / 2;
  const my = (s.y + t.y) / 2;

  return (
    <>
      <BaseEdge id={id} path={path} markerEnd={markerEnd} style={style} />
      <circle cx={s.x} cy={s.y} r={4} fill={color} stroke="var(--bg)" strokeWidth={1.5} />
      <circle cx={t.x} cy={t.y} r={4} fill={color} stroke="var(--bg)" strokeWidth={1.5} />
      <EdgeLabelRenderer>
        <button
          type="button"
          data-tooltip="Disconnect — stop this panel following the terminal"
          onClick={(e) => {
            e.stopPropagation();
            void deleteElements({ edges: [{ id }] });
          }}
          style={{
            position: "absolute",
            transform: `translate(-50%, -50%) translate(${mx}px, ${my}px)`,
            pointerEvents: "all",
          }}
          className="flex h-5 w-5 items-center justify-center rounded-full border border-[var(--border)] bg-[var(--surface)] text-xs leading-none text-[var(--text-dim)] shadow hover:bg-red-600 hover:text-white"
        >
          ×
        </button>
      </EdgeLabelRenderer>
    </>
  );
}
