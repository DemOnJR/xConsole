import { useEffect, useId, useRef, useState } from "react";

function sandColor(percent: number): string {
  if (percent >= 90) return "#f87171";
  if (percent >= 75) return "#fb923c";
  if (percent >= 50) return "#fbbf24";
  return "#a3e635";
}

/** Animated clepsydra — sand drains top→bottom as context fills; flips on compact. */
export function ContextHourglass({
  percent,
  compactFlipCount,
  size = 18,
  className = "",
}: {
  percent: number;
  compactFlipCount: number;
  size?: number;
  className?: string;
}) {
  const clipId = useId().replace(/:/g, "");
  const prevFlip = useRef(compactFlipCount);
  const [rotation, setRotation] = useState(0);
  const [flipping, setFlipping] = useState(false);

  useEffect(() => {
    if (compactFlipCount > prevFlip.current) {
      setFlipping(true);
      setRotation((r) => r + 180 * (compactFlipCount - prevFlip.current));
      prevFlip.current = compactFlipCount;
      const t = window.setTimeout(() => setFlipping(false), 720);
      return () => window.clearTimeout(t);
    }
  }, [compactFlipCount]);

  const fill = Math.min(100, Math.max(0, percent));
  const topLevel = (100 - fill) / 100;
  const bottomLevel = fill / 100;
  const sand = sandColor(fill);
  const flowing = fill > 2 && fill < 98;

  const topSandY = 4 + (1 - topLevel) * 7;
  const bottomSandH = bottomLevel * 7;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      className={`context-hourglass ${flipping ? "context-hourglass-flip" : ""} ${className}`}
      style={{ transform: `rotate(${rotation}deg)` }}
      aria-hidden
    >
      <defs>
        <clipPath id={`${clipId}-top`}>
          <path d="M7.5 3.5h9L12 11.5Z" />
        </clipPath>
        <clipPath id={`${clipId}-bottom`}>
          <path d="M7.5 20.5h9L12 12.5Z" />
        </clipPath>
        <clipPath id={`${clipId}-neck`}>
          <rect x="10.5" y="11" width="3" height="2.5" />
        </clipPath>
      </defs>

      {/* Frame */}
      <path
        d="M6.5 2.5h11v1.8L12 11.2 6.5 4.3V2.5zM6.5 21.5h11v-1.8L12 12.8 6.5 19.7V21.5z"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      <line x1="12" y1="11.2" x2="12" y2="12.8" stroke="currentColor" strokeWidth="1.2" opacity="0.5" />

      {/* Top sand */}
      <g clipPath={`url(#${clipId}-top)`}>
        <rect x="6" y={topSandY} width="12" height={12 - topSandY} fill={sand} opacity="0.92" />
      </g>

      {/* Bottom sand */}
      <g clipPath={`url(#${clipId}-bottom)`}>
        <rect
          x="6"
          y={20.5 - bottomSandH}
          width="12"
          height={bottomSandH + 0.5}
          fill={sand}
          opacity="0.92"
        />
      </g>

      {/* Falling grains in the neck */}
      {flowing && (
        <g clipPath={`url(#${clipId}-neck)`}>
          <circle className="context-hourglass-grain context-hourglass-grain-1" cx="12" cy="11.3" r="0.55" fill={sand} />
          <circle className="context-hourglass-grain context-hourglass-grain-2" cx="12" cy="11.8" r="0.45" fill={sand} />
          <circle className="context-hourglass-grain context-hourglass-grain-3" cx="12" cy="12.3" r="0.4" fill={sand} />
        </g>
      )}
    </svg>
  );
}
