/**
 * Human formatting for transfer stats. Pure and dependency-free so it can be checked
 * on its own — these strings update several times a second, and a wrong unit or a
 * jittery number is very visible.
 */

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** e.g. `1.4 MB`. Binary steps (1024), decimal-ish labels, as file managers show. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // One decimal below 10 keeps the width stable without looking falsely precise.
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${UNITS[unit]}`;
}

/** e.g. `4.2 MB/s`. */
export function formatRate(bytesPerSec: number): string {
  if (!Number.isFinite(bytesPerSec) || bytesPerSec <= 0) return "—";
  return `${formatBytes(bytesPerSec)}/s`;
}

/**
 * e.g. `0:07`, `3:21`, `1:04:09`. Always at least `m:ss` so the field doesn't change
 * width as seconds tick over.
 */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "—";
  const total = Math.round(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${ss}`;
  return `${m}:${ss}`;
}

/** Percentage 0–100. An unknown total reads as 0 rather than NaN. */
export function percent(done: number, total: number): number {
  if (!Number.isFinite(done) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, (done / total) * 100));
}
