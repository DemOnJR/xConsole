/** Cumulative CSS scale/zoom from ancestors (React Flow viewport, page zoom, etc.). */
export function getAccumulatedScale(el: HTMLElement): number {
  let scale = 1;
  let node: HTMLElement | null = el.parentElement;
  while (node) {
    const transform = getComputedStyle(node).transform;
    if (transform && transform !== "none") {
      const m = new DOMMatrixReadOnly(transform);
      scale *= m.a;
    }
    const zoom = (node.style as CSSStyleDeclaration & { zoom?: string }).zoom;
    if (zoom) scale *= parseFloat(zoom) || 1;
    node = node.parentElement;
  }
  const pageZoom = parseFloat(document.documentElement.style.zoom || "1") || 1;
  return scale * pageZoom;
}

/**
 * Counteract ancestor scale so xterm mouse/selection coords stay 1:1 with glyphs.
 * See: https://github.com/xtermjs/xterm.js/issues/6023
 */
export function applyXtermScaleFix(xtermEl: HTMLElement, scale: number): void {
  if (Math.abs(scale - 1) < 0.001) {
    xtermEl.style.zoom = "";
    xtermEl.style.width = "";
    xtermEl.style.height = "";
    return;
  }
  xtermEl.style.zoom = `${1 / scale}`;
  xtermEl.style.width = `${scale * 100}%`;
  xtermEl.style.height = `${scale * 100}%`;
}

export function findReactFlowViewport(el: HTMLElement): HTMLElement | null {
  let node: HTMLElement | null = el;
  while (node) {
    if (node.classList.contains("react-flow__viewport")) return node;
    node = node.parentElement;
  }
  return null;
}
