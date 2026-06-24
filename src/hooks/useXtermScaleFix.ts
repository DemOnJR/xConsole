import { useEffect, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";

// xterm decides which cell the mouse is over by dividing the on-screen pixel
// distance from the terminal's (CSS-scaled) bounding box by the UNSCALED css
// cell size. Under the canvas zoom (React Flow applies `transform: scale(z)` to
// the viewport) those two are in different units, so the selection/hit-test
// drifts proportionally to the zoom — exact only at 100%.
//
// Rather than fight the transform with a counter-scale (which distorts the
// renderer), we let the terminal scale visually like any node and instead patch
// xterm's internal mouse service so the cell/canvas dimensions it reads are
// multiplied by the live scale. That keeps hit-testing exact at any zoom while
// the text still shrinks/grows with the canvas.
// See: https://github.com/xtermjs/xterm.js/issues/6023

interface Dim {
  width: number;
  height: number;
}
type CoordFn = (...args: unknown[]) => unknown;
interface MouseSvc {
  getCoords: CoordFn;
  getMouseReportCoords?: CoordFn;
}
interface CoreInternals {
  _core?: {
    _mouseService?: MouseSvc;
    _renderService?: { dimensions?: { css?: { cell?: Dim; canvas?: Dim } } };
  };
}

/**
 * Keep xterm selection/mouse hit-testing aligned with the glyphs while the
 * terminal is rendered at an arbitrary CSS scale.
 *
 * @param termRef  the live Terminal instance
 * @param scaleRef the current on-screen scale of the terminal (1 = native).
 *                 In freeform mode this is the canvas zoom; in fixed layouts the
 *                 node counter-scales itself so it stays 1 (a no-op here).
 */
export function useXtermScaleFix(
  termRef: RefObject<Terminal | null>,
  scaleRef: RefObject<number>,
) {
  useEffect(() => {
    let raf = 0;
    let cancelled = false;
    let restore: (() => void) | undefined;

    const patch = (): boolean => {
      const term = termRef.current;
      const core = (term as unknown as CoreInternals | null)?._core;
      const mouse = core?._mouseService;
      const render = core?._renderService;
      // Both services are created during term.open(); retry until they exist.
      if (!mouse || !render) return false;

      const origCoords = mouse.getCoords.bind(mouse) as CoordFn;
      const origReport =
        typeof mouse.getMouseReportCoords === "function"
          ? (mouse.getMouseReportCoords.bind(mouse) as CoordFn)
          : undefined;

      // Run `fn` with the renderer's css cell/canvas dims temporarily scaled, so
      // xterm's own coordinate math (which reads them) produces correct cells.
      const withScale = (fn: CoordFn, args: unknown[]): unknown => {
        const scale = scaleRef.current || 1;
        if (Math.abs(scale - 1) < 0.0015) return fn(...args);
        let cell: Dim | undefined;
        let canvas: Dim | undefined;
        try {
          const css = render.dimensions?.css;
          cell = css?.cell;
          canvas = css?.canvas;
        } catch {
          /* renderer not ready */
        }
        if (!cell) return fn(...args);
        const cw = cell.width;
        const ch = cell.height;
        const kw = canvas?.width;
        const kh = canvas?.height;
        cell.width = cw * scale;
        cell.height = ch * scale;
        if (canvas && kw !== undefined && kh !== undefined) {
          canvas.width = kw * scale;
          canvas.height = kh * scale;
        }
        try {
          return fn(...args);
        } finally {
          cell.width = cw;
          cell.height = ch;
          if (canvas && kw !== undefined && kh !== undefined) {
            canvas.width = kw;
            canvas.height = kh;
          }
        }
      };

      mouse.getCoords = (...args: unknown[]) => withScale(origCoords, args);
      if (origReport) {
        mouse.getMouseReportCoords = (...args: unknown[]) => withScale(origReport, args);
      }

      restore = () => {
        mouse.getCoords = origCoords;
        if (origReport) mouse.getMouseReportCoords = origReport;
      };
      return true;
    };

    if (!patch()) {
      let tries = 0;
      const loop = () => {
        if (cancelled) return;
        if (patch() || tries++ > 180) return;
        raf = requestAnimationFrame(loop);
      };
      raf = requestAnimationFrame(loop);
    }

    return () => {
      cancelled = true;
      if (raf) cancelAnimationFrame(raf);
      restore?.();
    };
  }, [termRef, scaleRef]);
}
