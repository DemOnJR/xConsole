import React, { useMemo, type ReactNode } from "react";

interface GaugeProps {
  value: number; // 0 to 100
  label: string;
  sublabel?: string;
  size?: number;
  strokeWidth?: number;
  icon?: ReactNode;
  color?: string;
  formatValue?: (v: number) => string;
}

export const Gauge = React.memo(function Gauge({
  value,
  label,
  sublabel,
  size = 110,
  strokeWidth = 9,
  icon,
  color,
  formatValue,
}: GaugeProps) {
  const clamped = Math.max(0, Math.min(100, isNaN(value) ? 0 : value));
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference - (clamped / 100) * circumference;

  // Auto determine color if not provided
  const strokeColor = useMemo(() => {
    if (color) return color;
    if (clamped >= 85) return "#ef4444"; // Red
    if (clamped >= 65) return "#f59e0b"; // Amber
    return "#10b981"; // Emerald
  }, [color, clamped]);

  const formatted = useMemo(() => {
    return formatValue ? formatValue(clamped) : `${Math.round(clamped)}%`;
  }, [formatValue, clamped]);

  return (
    <div className="flex flex-col items-center justify-center p-3 rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)]/40 hover:border-[var(--accent,#06b6d4)]/40 transition-colors">
      <div className="relative flex items-center justify-center" style={{ width: size, height: size }}>
        <svg width={size} height={size} className="rotate-[-90deg]">
          {/* Background circle */}
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            stroke="var(--border,#27272a)"
            strokeWidth={strokeWidth}
            fill="none"
            opacity={0.5}
          />
          {/* Active progress arc */}
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            stroke={strokeColor}
            strokeWidth={strokeWidth}
            strokeDasharray={circumference}
            strokeDashoffset={offset}
            strokeLinecap="round"
            fill="none"
            className="transition-all duration-300 ease-out"
            style={{ willChange: "stroke-dashoffset" }}
          />
        </svg>

        {/* Center content */}
        <div className="absolute inset-0 flex flex-col items-center justify-center text-center select-none">
          {icon && <div className="mb-0.5 text-[var(--text-faint,#71717a)]">{icon}</div>}
          <span className="font-mono text-base font-bold tracking-tight text-[var(--text,#fafafa)]">
            {formatted}
          </span>
        </div>
      </div>

      <div className="mt-2 text-center select-none">
        <div className="text-xs font-medium text-[var(--text,#e4e4e7)]">{label}</div>
        {sublabel && (
          <div className="text-[11px] font-mono text-[var(--text-faint,#71717a)]">{sublabel}</div>
        )}
      </div>
    </div>
  );
});
