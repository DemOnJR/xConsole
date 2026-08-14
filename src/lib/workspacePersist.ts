/** Stable persist key for the workspace autosave. Ignores focus, selection, and subpixel jitter. */

export type PersistViewport = { x: number; y: number; zoom: number };

export type PersistableCanvas = {
  nodes: Array<{
    id: string;
    type?: string;
    position: { x: number; y: number };
    width?: number | null;
    height?: number | null;
    data: Record<string, unknown>;
  }>;
  edges: Array<{ source: string; target: string }>;
  layoutMode: string;
  tileLayout: unknown;
};

const px = (n: number) => Math.round(n);
const q2 = (n: number) => Math.round(n * 100) / 100;
const q4 = (n: number) => Math.round(n * 10_000) / 10_000;

/** JSON key of the layout that is worth writing to disk. */
export function workspacePersistKey(
  canvas: PersistableCanvas,
  viewport: PersistViewport,
): string {
  return JSON.stringify({
    nodes: canvas.nodes.map((n) => [
      n.id,
      n.type ?? "",
      px(n.position.x),
      px(n.position.y),
      px(Number(n.width) || 0),
      px(Number(n.height) || 0),
      String(n.data.vpsId ?? ""),
      String(n.data.name ?? ""),
      String(n.data.host ?? ""),
      String(n.data.linkedTerminalId ?? ""),
      n.data.followTerminal === true ? 1 : 0,
      String(n.data.goalId ?? ""),
    ]),
    edges: canvas.edges.map((e) => [e.source, e.target]),
    layout: canvas.layoutMode,
    tiles: canvas.tileLayout,
    vp: [q2(viewport.x), q2(viewport.y), q4(viewport.zoom)],
  });
}

/** Round a live viewport so a 1e-10 pan does not rewrite the workspace row. */
export function stableViewport(viewport: PersistViewport): PersistViewport {
  return { x: q2(viewport.x), y: q2(viewport.y), zoom: q4(viewport.zoom) };
}
