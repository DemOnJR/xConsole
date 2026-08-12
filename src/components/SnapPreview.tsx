import { useSnapDragStore, activeSnapZone } from "../lib/snapDrag";
import { snapZones } from "../lib/snapLayout";

/**
 * Windows-style snap-layout preview. Rendered inside the React Flow viewport (so it
 * inherits the pane coordinate system and zoom). While a node is being dragged, it
 * draws the translucent zones — but only once the cursor has entered a zone's trigger
 * band (Windows only shows the layout when you get close to a snap position), and it
 * highlights the zone currently under the cursor.
 */
export function SnapPreview() {
  const armed = useSnapDragStore((s) => s.armed);
  const count = useSnapDragStore((s) => s.count);

  if (!armed) return null;
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
