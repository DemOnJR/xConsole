import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

// Global tooltip layer. Any element with a `data-tooltip="..."` attribute shows a
// themed tooltip on hover/focus, rendered into <body> so it never clips. It
// auto-places on whichever side has room (and is fully clamped into the viewport),
// so it's never cut off at an edge. `data-tooltip-side="top|bottom|left|right"`
// sets a preferred side (e.g. "right" for items in a narrow collapsed sidebar).

interface Anchor {
  rect: DOMRect;
  text: string;
  side: string;
}

const SHOW_DELAY = 250;

export function TooltipHost() {
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer: number | undefined;
    let current: HTMLElement | null = null;

    const clear = () => {
      if (timer) window.clearTimeout(timer);
      timer = undefined;
      current = null;
      setAnchor(null);
      setPos(null);
    };

    const show = (el: HTMLElement) => {
      const text = el.getAttribute("data-tooltip");
      if (!text) return;
      setPos(null); // re-measure for this tooltip
      setAnchor({
        rect: el.getBoundingClientRect(),
        text,
        side: el.getAttribute("data-tooltip-side") || "auto",
      });
    };

    const onOver = (e: MouseEvent) => {
      const el = (e.target as HTMLElement | null)?.closest?.("[data-tooltip]") as HTMLElement | null;
      if (!el || el === current) return;
      current = el;
      if (timer) window.clearTimeout(timer);
      timer = window.setTimeout(() => show(el), SHOW_DELAY);
    };
    const onOut = (e: MouseEvent) => {
      const related = e.relatedTarget as HTMLElement | null;
      if (current && related && current.contains(related)) return;
      clear();
    };

    document.addEventListener("mouseover", onOver, true);
    document.addEventListener("mouseout", onOut, true);
    document.addEventListener("mousedown", clear, true);
    window.addEventListener("scroll", clear, true);
    window.addEventListener("blur", clear);

    return () => {
      if (timer) window.clearTimeout(timer);
      document.removeEventListener("mouseover", onOver, true);
      document.removeEventListener("mouseout", onOut, true);
      document.removeEventListener("mousedown", clear, true);
      window.removeEventListener("scroll", clear, true);
      window.removeEventListener("blur", clear);
    };
  }, []);

  // Measure the rendered tooltip, then place it on a side that fits and clamp it
  // fully into the viewport so it is never cut off.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!anchor || !el) return;
    const tw = el.offsetWidth;
    const th = el.offsetHeight;
    const r = anchor.rect;
    const W = window.innerWidth;
    const H = window.innerHeight;
    const GAP = 8;
    const M = 6; // min margin from the viewport edge

    const candidate = (side: string) => {
      switch (side) {
        case "top":
          return { left: r.left + r.width / 2 - tw / 2, top: r.top - GAP - th, fits: r.top - GAP - th >= M };
        case "bottom":
          return { left: r.left + r.width / 2 - tw / 2, top: r.bottom + GAP, fits: r.bottom + GAP + th <= H - M };
        case "right":
          return { left: r.right + GAP, top: r.top + r.height / 2 - th / 2, fits: r.right + GAP + tw <= W - M };
        case "left":
          return { left: r.left - GAP - tw, top: r.top + r.height / 2 - th / 2, fits: r.left - GAP - tw >= M };
        default:
          return { left: 0, top: 0, fits: false };
      }
    };

    const order =
      anchor.side !== "auto"
        ? [anchor.side, "bottom", "top", "right", "left"]
        : ["top", "bottom", "right", "left"];
    const chosen = order.map(candidate).find((c) => c.fits) ?? candidate(order[0]);

    const left = Math.min(Math.max(chosen.left, M), Math.max(M, W - tw - M));
    const top = Math.min(Math.max(chosen.top, M), Math.max(M, H - th - M));
    setPos({ left, top });
  }, [anchor]);

  if (!anchor) return null;

  return createPortal(
    <div
      ref={ref}
      role="tooltip"
      style={{
        position: "fixed",
        left: pos?.left ?? -9999,
        top: pos?.top ?? -9999,
        visibility: pos ? "visible" : "hidden",
        zIndex: 9999,
        pointerEvents: "none",
        maxWidth: 260,
        width: "max-content",
        padding: "5px 9px",
        borderRadius: 6,
        fontSize: 11,
        lineHeight: 1.35,
        background: "var(--surface-2, var(--surface))",
        color: "var(--text)",
        border: "1px solid var(--border)",
        boxShadow: "0 6px 18px rgba(0,0,0,0.35)",
        whiteSpace: "normal",
      }}
    >
      {anchor.text}
    </div>,
    document.body,
  );
}
