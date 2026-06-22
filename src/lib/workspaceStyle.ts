import type { CSSProperties } from "react";
import type { ColorMode } from "./tauri";

export const DEFAULT_COLOR = "#3b82f6";
export const DEFAULT_ICON = "🖥️";
export const DEFAULT_COLOR_MODE: ColorMode = "side";

export const COLOR_MODES: { id: ColorMode; label: string }[] = [
  { id: "side", label: "Side" },
  { id: "border", label: "Border" },
  { id: "bg", label: "Background" },
];

/** Convert a #rrggbb hex to an rgba() string with the given alpha. */
export function hexToRgba(hex: string, alpha: number): string {
  const h = hex.replace("#", "");
  const full =
    h.length === 3
      ? h
          .split("")
          .map((c) => c + c)
          .join("")
      : h;
  const n = parseInt(full, 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/**
 * Style for a workspace control given its accent color, where the color should
 * be applied, and whether it is currently active.
 */
export function accentStyle(
  color: string,
  mode: ColorMode,
  active: boolean,
): CSSProperties {
  switch (mode) {
    case "bg":
      return {
        backgroundColor: hexToRgba(color, active ? 0.32 : 0.16),
        borderColor: active ? hexToRgba(color, 0.5) : "transparent",
      };
    case "border":
      return {
        borderColor: color,
        boxShadow: active ? `0 0 0 1px ${color}` : undefined,
      };
    case "side":
    default:
      return {
        boxShadow: `inset 3px 0 0 ${color}`,
        backgroundColor: active ? hexToRgba(color, 0.1) : undefined,
      };
  }
}
