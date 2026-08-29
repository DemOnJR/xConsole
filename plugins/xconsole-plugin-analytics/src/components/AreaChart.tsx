import React, { useState, useId, useMemo, useCallback } from "react";

interface AreaChartProps {
  data: number[];
  labels?: string[];
  height?: number;
  color?: string;
  fillColor?: string;
  unit?: string;
  title?: string;
  subtitle?: string;
  minLabel?: string;
  maxLabel?: string;
  showGrid?: boolean;
  valueFormatter?: (v: number) => string;
}

export const AreaChart = React.memo(function AreaChart({
  data,
  labels,
  height = 140,
  color = "var(--accent, #06b6d4)",
  fillColor,
  unit = "",
  title,
  subtitle,
  showGrid = true,
  valueFormatter,
}: AreaChartProps) {
  const chartId = useId();
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const w = 600;
  const h = height;
  const padTop = 12;
  const padBottom = 20;
  const padLeft = 8;
  const padRight = 8;
  const plotWidth = w - padLeft - padRight;
  const plotHeight = h - padTop - padBottom;

  const chartMetrics = useMemo(() => {
    if (!data || data.length === 0) {
      return null;
    }

    const min = Math.min(...data);
    const max = Math.max(...data);
    const avg = data.reduce((a, b) => a + b, 0) / data.length;
    const span = Math.max(1e-4, max - min);

    const points = data.map((v, i) => {
      const x = padLeft + (i / Math.max(1, data.length - 1)) * plotWidth;
      const y = padTop + plotHeight - ((v - min) / span) * plotHeight;
      return { x, y, value: v, label: labels?.[i] };
    });

    const polylinePts = points.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ");
    const areaPath = `M ${points[0]?.x ?? 0} ${padTop + plotHeight} L ${polylinePts} L ${points[points.length - 1]?.x ?? 0} ${padTop + plotHeight} Z`;

    return { min, max, avg, points, polylinePts, areaPath };
  }, [data, labels, height, plotWidth, plotHeight]);

  const handleMouseMove = useCallback((e: React.MouseEvent<SVGSVGElement>) => {
    if (!data || data.length === 0) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const clientX = e.clientX - rect.left;
    const normX = Math.max(0, Math.min(1, (clientX - padLeft) / plotWidth));
    const idx = Math.round(normX * (data.length - 1));
    if (idx >= 0 && idx < data.length) {
      setHoverIndex((prev) => (prev === idx ? prev : idx));
    }
  }, [data, plotWidth]);

  const handleMouseLeave = useCallback(() => {
    setHoverIndex(null);
  }, []);

  if (!chartMetrics) {
    return (
      <div className="flex h-36 items-center justify-center text-xs text-[var(--text-faint,#71717a)]">
        No telemetry samples collected yet
      </div>
    );
  }

  const { max, avg, points, polylinePts, areaPath } = chartMetrics;
  const format = valueFormatter || ((v: number) => `${v.toFixed(1)}${unit ? ` ${unit}` : ""}`);
  const activePoint = hoverIndex !== null ? points[hoverIndex] : null;

  return (
    <div className="flex flex-col gap-2 select-none" style={{ contain: "content" }}>
      {(title || subtitle) && (
        <div className="flex items-baseline justify-between gap-2">
          <div>
            {title && <h3 className="text-xs font-semibold text-[var(--text,#e4e4e7)]">{title}</h3>}
            {subtitle && <p className="text-[11px] text-[var(--text-faint,#71717a)]">{subtitle}</p>}
          </div>
          <div className="flex items-center gap-3 font-mono text-[11px] text-[var(--text-faint,#a1a1aa)]">
            <span>
              Avg: <strong className="text-[var(--text,#fafafa)]">{format(avg)}</strong>
            </span>
            <span>
              Max: <strong className="text-[var(--text,#fafafa)]">{format(max)}</strong>
            </span>
          </div>
        </div>
      )}

      <div className="relative w-full overflow-hidden rounded-lg bg-[var(--surface-2,#18181b)]/50 p-2 border border-[var(--border,#27272a)]">
        <svg
          width="100%"
          height={height}
          viewBox={`0 0 ${w} ${h}`}
          preserveAspectRatio="none"
          className="block overflow-visible cursor-crosshair"
          shapeRendering="geometricPrecision"
          onMouseMove={handleMouseMove}
          onMouseLeave={handleMouseLeave}
        >
          <defs>
            <linearGradient id={`grad-${chartId}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={fillColor || color} stopOpacity="0.38" />
              <stop offset="90%" stopColor={fillColor || color} stopOpacity="0.02" />
            </linearGradient>
          </defs>

          {/* Grid lines */}
          {showGrid && (
            <g stroke="var(--border,#27272a)" strokeWidth="1" strokeDasharray="3 3" opacity="0.6">
              <line x1={padLeft} y1={padTop} x2={w - padRight} y2={padTop} />
              <line x1={padLeft} y1={padTop + plotHeight / 2} x2={w - padRight} y2={padTop + plotHeight / 2} />
              <line x1={padLeft} y1={padTop + plotHeight} x2={w - padRight} y2={padTop + plotHeight} />
            </g>
          )}

          {/* Area fill */}
          <path d={areaPath} fill={`url(#grad-${chartId})`} />

          {/* Line stroke */}
          <polyline
            fill="none"
            stroke={color}
            strokeWidth="2"
            strokeLinejoin="round"
            strokeLinecap="round"
            points={polylinePts}
          />

          {/* Hover Crosshair & Dot */}
          {activePoint && (
            <g>
              <line
                x1={activePoint.x}
                y1={padTop}
                x2={activePoint.x}
                y2={padTop + plotHeight}
                stroke="var(--accent,#06b6d4)"
                strokeWidth="1.5"
                strokeDasharray="2 2"
                opacity="0.85"
              />
              <circle
                cx={activePoint.x}
                cy={activePoint.y}
                r="4.5"
                fill="var(--surface,#18181b)"
                stroke={color}
                strokeWidth="2.5"
              />
            </g>
          )}
        </svg>

        {/* Hover Tooltip Overlay */}
        {activePoint && (
          <div
            className="pointer-events-none absolute z-10 -translate-x-1/2 -translate-y-full rounded-md border border-[var(--border,#3f3f46)] bg-[var(--surface,#09090b)] px-2.5 py-1 text-xs shadow-xl"
            style={{
              left: `${(activePoint.x / w) * 100}%`,
              top: `${Math.max(24, (activePoint.y / h) * 100)}%`,
            }}
          >
            <div className="font-mono font-semibold text-[var(--text,#fafafa)]">
              {format(activePoint.value)}
            </div>
            {activePoint.label && (
              <div className="text-[10px] text-[var(--text-faint,#a1a1aa)]">
                {activePoint.label}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
});
