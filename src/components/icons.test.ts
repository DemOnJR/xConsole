import { describe, expect, it } from "vitest";
// Read through Vite rather than `node:fs`: the project has no Node types, and `?raw`
// resolves the same file the app imports, so the test cannot drift onto a stale copy.
import SRC from "./icons.tsx?raw";

/**
 * Icons are drawn by hand on a shared 24x24 grid, and nothing about that grid is
 * enforced by the type system: an icon whose path wanders outside it renders clipped,
 * and one drawn off to a corner renders visibly smaller than its neighbours at the same
 * `size`. Both had happened — the refresh arrow ended at x=26.4 and was simply cut off,
 * and the puzzle piece sat 5.7 from the left edge against 2.4 from the right.
 *
 * So this measures the ink box of every icon in the file. It is a coarse instrument on
 * purpose: curves are approximated by their control points, which overestimates, so the
 * bounds are loose enough that only a real mistake trips them.
 */

/** The grid every icon is drawn on. */
const BOX = 24;

type Point = [number, number];

/**
 * Centre of the ellipse an arc is drawn on (SVG spec F.6.5 endpoint parameterisation).
 *
 * Needed only for large-arc sweeps, where the endpoints say almost nothing about where
 * the curve actually goes.
 */
function arcCentre(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  rx: number,
  ry: number,
  rotationDeg: number,
  largeArc: number,
  sweep: number,
): [number, number] | null {
  // These icons draw circles and axis-aligned ellipses; a rotated one would need the
  // full transform, and there are none.
  if (rotationDeg !== 0) return null;
  const dx = (x1 - x2) / 2;
  const dy = (y1 - y2) / 2;
  // Radii too small to span the chord are scaled up, per the spec — otherwise the
  // square root below goes imaginary.
  const lambda = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry);
  const s = lambda > 1 ? Math.sqrt(lambda) : 1;
  const RX = rx * s;
  const RY = ry * s;
  const denom = RX * RX * dy * dy + RY * RY * dx * dx;
  if (denom === 0) return null;
  const numerator = RX * RX * RY * RY - denom;
  // Two ellipses pass through both endpoints with these radii; the spec picks between
  // them with *both* flags — negative when they agree. Using only the large-arc flag
  // puts the centre on the wrong side, which is worse than not computing it at all.
  const coef = Math.sqrt(Math.max(0, numerator / denom)) * (largeArc !== sweep ? 1 : -1);
  const cxp = (coef * RX * dy) / RY;
  const cyp = (-coef * RY * dx) / RX;
  return [cxp + (x1 + x2) / 2, cyp + (y1 + y2) / 2];
}

/** Control points of a path's `d`, which bound the curve it draws. */
function pathPoints(d: string): Point[] {
  const tokens = d.match(/[MmLlHhVvCcSsQqTtAaZz]|-?\d*\.?\d+(?:e-?\d+)?/g) ?? [];
  const pts: Point[] = [];
  let i = 0;
  let cx = 0;
  let cy = 0;
  let sx = 0;
  let sy = 0;
  let cmd = "";
  const num = () => Number(tokens[i++]);

  while (i < tokens.length) {
    if (/^[A-Za-z]$/.test(tokens[i])) {
      cmd = tokens[i++];
      if (cmd === "Z" || cmd === "z") {
        cx = sx;
        cy = sy;
        continue;
      }
    }
    const rel = cmd === cmd.toLowerCase();
    const c = cmd.toUpperCase();

    if (c === "M" || c === "L") {
      let x = num();
      let y = num();
      if (rel) {
        x += cx;
        y += cy;
      }
      cx = x;
      cy = y;
      if (c === "M") {
        sx = x;
        sy = y;
        // A moveto's implicit continuation is a lineto, per the SVG grammar.
        cmd = rel ? "l" : "L";
      }
      pts.push([cx, cy]);
    } else if (c === "H") {
      const x = num();
      cx = rel ? cx + x : x;
      pts.push([cx, cy]);
    } else if (c === "V") {
      const y = num();
      cy = rel ? cy + y : y;
      pts.push([cx, cy]);
    } else if (c === "C" || c === "S" || c === "Q" || c === "T") {
      const n = { C: 6, S: 4, Q: 4, T: 2 }[c] as number;
      const vals = Array.from({ length: n }, num);
      for (let k = 0; k < n; k += 2) {
        const x = rel ? vals[k] + cx : vals[k];
        const y = rel ? vals[k + 1] + cy : vals[k + 1];
        pts.push([x, y]);
        if (k === n - 2) {
          cx = x;
          cy = y;
        }
      }
    } else if (c === "A") {
      // rx ry rotation large-arc sweep x y.
      const rx = Math.abs(num());
      const ry = Math.abs(num());
      const rot = num();
      const largeArc = num();
      const sweep = num();
      let x = num();
      let y = num();
      if (rel) {
        x += cx;
        y += cy;
      }
      pts.push([cx, cy], [x, y]);
      // A large-arc sweep covers more than half the ellipse, so it reaches at least
      // two of its axis extremes — and in this file, always close to all four. The
      // endpoints alone would put a three-quarter spinner in one corner of the grid
      // and report it as badly off-centre when it is a perfect circle.
      if (largeArc === 1 && rx > 0 && ry > 0) {
        const centre = arcCentre(cx, cy, x, y, rx, ry, rot, largeArc, sweep);
        if (centre) {
          pts.push(
            [centre[0] - rx, centre[1] - ry],
            [centre[0] + rx, centre[1] + ry],
          );
        }
      }
      cx = x;
      cy = y;
    } else {
      i++;
    }
  }
  return pts;
}

/** Every point that contributes ink, across all the shapes in one icon's body. */
function inkPoints(body: string): Point[] {
  const pts: Point[] = [];
  const attr = (attrs: string, name: string): number | null => {
    const m = attrs.match(new RegExp(`${name}="(-?[\\d.]+)"`));
    return m ? Number(m[1]) : null;
  };

  for (const m of body.matchAll(
    /<(rect|circle|ellipse|line|polyline|polygon|path)\b([^>]*)>/g,
  )) {
    const [, tag, attrs] = m;
    if (tag === "rect") {
      const x = attr(attrs, "x");
      const y = attr(attrs, "y");
      const w = attr(attrs, "width");
      const h = attr(attrs, "height");
      if (x != null && y != null && w != null && h != null) {
        pts.push([x, y], [x + w, y + h]);
      }
    } else if (tag === "circle" || tag === "ellipse") {
      const cx = attr(attrs, "cx");
      const cy = attr(attrs, "cy");
      const rx = attr(attrs, "r") ?? attr(attrs, "rx");
      const ry = attr(attrs, "r") ?? attr(attrs, "ry");
      if (cx != null && cy != null && rx != null && ry != null) {
        pts.push([cx - rx, cy - ry], [cx + rx, cy + ry]);
      }
    } else if (tag === "line") {
      const x1 = attr(attrs, "x1");
      const y1 = attr(attrs, "y1");
      const x2 = attr(attrs, "x2");
      const y2 = attr(attrs, "y2");
      if (x1 != null && y1 != null && x2 != null && y2 != null) {
        pts.push([x1, y1], [x2, y2]);
      }
    } else if (tag === "polyline" || tag === "polygon") {
      const raw = attrs.match(/points="([^"]+)"/);
      if (raw) {
        const n = (raw[1].match(/-?[\d.]+/g) ?? []).map(Number);
        for (let k = 0; k + 1 < n.length; k += 2) pts.push([n[k], n[k + 1]]);
      }
    } else if (tag === "path") {
      const raw = attrs.match(/d="([^"]+)"/);
      if (raw) pts.push(...pathPoints(raw[1]));
    }
  }
  return pts;
}

interface Icon {
  name: string;
  x0: number;
  x1: number;
  y0: number;
  y1: number;
}

function icons(): Icon[] {
  const out: Icon[] = [];
  for (const part of SRC.split("\nexport function ").slice(1)) {
    const name = part.split("(")[0];
    if (!name.endsWith("Icon")) continue;
    const pts = inkPoints(part.split("</svg>")[0]);
    if (pts.length === 0) continue;
    const xs = pts.map((p) => p[0]);
    const ys = pts.map((p) => p[1]);
    out.push({
      name,
      x0: Math.min(...xs),
      x1: Math.max(...xs),
      y0: Math.min(...ys),
      y1: Math.max(...ys),
    });
  }
  return out;
}

describe("icon geometry", () => {
  const all = icons();

  it("finds the icons", () => {
    // A parser that silently matched nothing would make every assertion below vacuous.
    expect(all.length).toBeGreaterThan(60);
  });

  it("keeps every icon inside the 24x24 grid", () => {
    // Anything outside is clipped by the viewBox, which reads as an icon that is
    // mysteriously smaller or missing a stroke rather than as a bug.
    const escaped = all.filter(
      (i) => i.x0 < -0.5 || i.y0 < -0.5 || i.x1 > BOX + 0.5 || i.y1 > BOX + 0.5,
    );
    expect(escaped.map((i) => `${i.name} ${i.x0},${i.y0}..${i.x1},${i.y1}`)).toEqual([]);
  });

  it("keeps every icon roughly centred on the grid", () => {
    // Two icons at the same `size` occupy the same box, so one drawn toward a corner
    // renders visibly smaller than its neighbour. Three units of slack: enough for a
    // deliberately asymmetric shape, not enough to hide a piece shoved to one side.
    const off = all
      .map((i) => ({
        name: i.name,
        dx: (i.x0 + i.x1) / 2 - BOX / 2,
        dy: (i.y0 + i.y1) / 2 - BOX / 2,
      }))
      .filter((i) => Math.abs(i.dx) > 3 || Math.abs(i.dy) > 3);
    expect(off.map((i) => `${i.name} off by ${i.dx.toFixed(1)},${i.dy.toFixed(1)}`)).toEqual(
      [],
    );
  });

  it("draws every icon big enough to read", () => {
    // An icon occupying a third of the grid looks like a smaller icon, whatever `size`
    // it is given.
    const tiny = all.filter((i) => Math.max(i.x1 - i.x0, i.y1 - i.y0) < 8);
    expect(tiny.map((i) => i.name)).toEqual([]);
  });
});
