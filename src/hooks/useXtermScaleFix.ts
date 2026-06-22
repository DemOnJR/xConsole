import { useEffect, type RefObject } from "react";
import { useStore } from "@xyflow/react";
import type { FitAddon } from "@xterm/addon-fit";
import {
  applyXtermScaleFix,
  findReactFlowViewport,
  getAccumulatedScale,
} from "../lib/xtermScaleFix";

/** Keep xterm selection aligned when embedded under CSS scale transforms. */
export function useXtermScaleFix(
  containerRef: RefObject<HTMLElement | null>,
  fitRef: RefObject<FitAddon | null>,
  enabled = true,
) {
  const viewportZoom = useStore((s) => s.transform[2]);

  useEffect(() => {
    if (!enabled) return;
    const container = containerRef.current;
    if (!container) return;

    const sync = () => {
      const xtermEl = container.querySelector(".xterm") as HTMLElement | null;
      if (!xtermEl) return;
      const scale = getAccumulatedScale(xtermEl);
      applyXtermScaleFix(xtermEl, scale);
      try {
        fitRef.current?.fit();
      } catch {
        /* not ready */
      }
    };

    sync();
    const ro = new ResizeObserver(sync);
    ro.observe(container);

    const viewport = findReactFlowViewport(container);
    const mo = new MutationObserver(sync);
    if (viewport) {
      mo.observe(viewport, { attributes: true, attributeFilter: ["style"] });
    }

    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  }, [containerRef, fitRef, enabled, viewportZoom]);
}
