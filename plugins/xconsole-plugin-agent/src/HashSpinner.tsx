import { activityKind, motionForKind, type ActivityKind, type HashMotion } from "../../../src/lib/activityMotion";
import type { AgentActivityItem } from "../../../src/stores/agentStore";

/** Four strokes of a #. CSS moves each bar into figures, then back. */
export function HashSpinner({
  kind,
  item,
  motion,
}: {
  kind?: ActivityKind;
  item?: Pick<AgentActivityItem, "kind" | "tool" | "label" | "state" | "path">;
  motion?: HashMotion;
}) {
  const resolved = kind ?? activityKind(item);
  const style = motion ?? motionForKind(resolved);
  return (
    <span className={`xc-hash xc-hash--${resolved} xc-hash-fig--${style}`} aria-hidden>
      <svg viewBox="0 0 16 16" width="13" height="13">
        <rect className="xc-hash-bar xc-hash-bar--vl" x="7.2" y="1" width="1.6" height="14" rx="0.55" />
        <rect className="xc-hash-bar xc-hash-bar--vr" x="7.2" y="1" width="1.6" height="14" rx="0.55" />
        <rect className="xc-hash-bar xc-hash-bar--ht" x="7.2" y="1" width="1.6" height="14" rx="0.55" />
        <rect className="xc-hash-bar xc-hash-bar--hb" x="7.2" y="1" width="1.6" height="14" rx="0.55" />
      </svg>
    </span>
  );
}
