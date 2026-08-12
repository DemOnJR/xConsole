import { useSnapDragStore, activeSnapZone } from "../lib/snapDrag";
import { snapZones } from "../lib/snapLayout";

/**
 * Windows-style snap-layout preview. Rendered inside the React Flow viewport (so it
 * inherits the pane coordinate system and zoom), it draws the translucent zones while
 * a node is being dragged in freeform mode, highlighting the one under the cursor.
 */
export function SnapPreview() {
  const dragging = useSnapDragStore((s) => s.nodeId !== null);
  const count = useSnapDragStore((s) => s.count);

  if (!dragging) return null;
  const zones = snapZones(count);
  const active = activeSnapZone();

  return (
    <div className="pointer-events-none absolute inset-0 z-40">
      {zones.map((zone) => {
        const isActive = active?.id === zone.id;
        return (
          <div
            key={zone.id}
            className="absolute rounded-md border"
            style={{
              left: `${zone.x * 100}%`,
              top: `${zone.y * 100}%`,
              width: `${zone.w * 100}%`,
              height: `${zone.h * 100}%`,
              background: isActive
                ? "rgba(59, 130, 246, 0.25)"
                : "rgba(59, 130, 246, 0.08)",
              borderColor: isActive ? "rgba(96, 165, 250, 0.8)" : "rgba(96, 165, 250, 0.25)",
            }}
          />
        );
      })}
    </div>
  );
}
