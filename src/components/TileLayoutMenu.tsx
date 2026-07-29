import { useEffect, useRef, useState } from "react";
import { useCanvasStore } from "../stores/canvasStore";
import {
  defaultRowCounts,
  parseRowCounts,
  reconcile,
  rowCounts,
  type TileLayout,
} from "../lib/tileLayout";
import { RowsIcon } from "./icons";

const ICON_BTN =
  "flex items-center justify-center rounded-md border border-[var(--border)] p-1.5 text-gray-300 hover:bg-[var(--border)] hover:text-white";

const STEP_BTN =
  "flex h-5 w-5 items-center justify-center rounded border border-[var(--border)] text-xs leading-none text-gray-300 hover:bg-[var(--border)] hover:text-white disabled:cursor-not-allowed disabled:opacity-30";

/** A handful of shapes worth one click, for `n` tiles. */
function presets(n: number): number[][] {
  if (n <= 1) return [];
  const seen = new Set<string>();
  const out: number[][] = [];
  const add = (counts: number[]) => {
    const usable = counts.filter((c) => c > 0);
    if (usable.reduce((a, b) => a + b, 0) !== n) return;
    const key = usable.join(",");
    if (seen.has(key)) return;
    seen.add(key);
    out.push(usable);
  };

  add(defaultRowCounts(n)); // the balanced default first
  add([n]); // one row across
  // Every even-ish split into 2..4 rows, extras on top (the shape people expect).
  for (let rows = 2; rows <= Math.min(4, n); rows++) {
    const base = Math.floor(n / rows);
    const extra = n % rows;
    if (base === 0) continue;
    add(Array.from({ length: rows }, (_, r) => base + (r < extra ? 1 : 0)));
  }
  add(Array.from({ length: n }, () => 1)); // one per row (a full-width stack)
  return out.slice(0, 6);
}

/**
 * The tile-shape editor: pick how many tiles sit in each row.
 *
 * Rows are the whole model — a row always fills the full width and splits it between
 * the tiles it holds, so "3 on top, 2 on the bottom, both full width" is the shape
 * `3, 2`, and a row holding one tile spans the width on its own.
 */
export function TileLayoutMenu() {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const wrapRef = useRef<HTMLDivElement | null>(null);

  const nodes = useCanvasStore((s) => s.nodes);
  const tileLayout = useCanvasStore((s) => s.tileLayout);
  const setTileRows = useCanvasStore((s) => s.setTileRows);
  const resetTileLayout = useCanvasStore((s) => s.resetTileLayout);

  const n = nodes.length;
  // Reconcile before reading the shape: the stored layout can lag the node list by a
  // render (or come from a previous session) and would otherwise show a stale count.
  const layout: TileLayout = reconcile(tileLayout, nodes.map((node) => node.id));
  const counts = rowCounts(layout);
  const shape = counts.join(", ");

  useEffect(() => {
    if (open) setDraft(shape);
  }, [open, shape]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const parsed = parseRowCounts(draft);
  const draftTotal = parsed?.reduce((a, b) => a + b, 0) ?? 0;
  const draftValid = parsed !== null && draftTotal === n;

  /** Move one tile between two rows, keeping the total constant. */
  const bump = (row: number, delta: number) => {
    const next = [...counts];
    const donor = delta > 0 ? next.findIndex((c, i) => i !== row && c > 1) : -1;
    if (delta > 0) {
      // Pull a tile in from another row so the total never changes.
      if (donor === -1) return;
      next[donor] -= 1;
      next[row] += 1;
    } else {
      if (next[row] <= 1) return;
      next[row] -= 1;
      // Push it to the next row down, or start a new bottom row.
      if (row + 1 < next.length) next[row + 1] += 1;
      else next.push(1);
    }
    setTileRows(next.filter((c) => c > 0));
  };

  const setRowTotal = (rows: number) => {
    if (rows < 1 || rows > n) return;
    const base = Math.floor(n / rows);
    const extra = n % rows;
    setTileRows(Array.from({ length: rows }, (_, r) => base + (r < extra ? 1 : 0)));
  };

  return (
    <div className="relative" ref={wrapRef}>
      <button
        data-tooltip="Tile shape — how many terminals per row"
        onClick={() => setOpen((v) => !v)}
        className={`${ICON_BTN} ${open ? "bg-[var(--border)] text-white" : ""}`}
      >
        <RowsIcon size={15} />
      </button>

      {open ? (
        <div className="absolute left-1/2 top-full z-50 mt-2 w-72 -translate-x-1/2 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3 shadow-xl">
          {n === 0 ? (
            <p className="text-xs text-gray-500">Add a terminal to the canvas first.</p>
          ) : (
            <>
              <div className="mb-2 flex items-baseline justify-between">
                <span className="text-xs font-medium text-gray-200">Tiles per row</span>
                <span className="text-[11px] text-gray-500">
                  {n} tile{n === 1 ? "" : "s"}
                </span>
              </div>

              {/* Live preview of the shape. */}
              <div className="mb-3 flex flex-col gap-[3px] rounded border border-[var(--border)] bg-[var(--bg)] p-1.5">
                {counts.map((c, r) => (
                  <div key={r} className="flex gap-[3px]" style={{ height: 14 }}>
                    {Array.from({ length: c }, (_, i) => (
                      <div key={i} className="flex-1 rounded-[2px] bg-blue-600/40" />
                    ))}
                  </div>
                ))}
              </div>

              {/* Per-row steppers. */}
              <div className="mb-3 flex flex-col gap-1.5">
                {counts.map((c, r) => (
                  <div key={r} className="flex items-center gap-2 text-[11px] text-gray-400">
                    <span className="w-10 shrink-0">Row {r + 1}</span>
                    <button
                      className={STEP_BTN}
                      disabled={c <= 1}
                      onClick={() => bump(r, -1)}
                      data-tooltip="One fewer tile in this row"
                    >
                      −
                    </button>
                    <span className="w-4 text-center tabular-nums text-gray-200">{c}</span>
                    <button
                      className={STEP_BTN}
                      disabled={!counts.some((other, i) => i !== r && other > 1)}
                      onClick={() => bump(r, 1)}
                      data-tooltip="One more tile in this row"
                    >
                      +
                    </button>
                  </div>
                ))}
              </div>

              {/* Row-count shortcut. */}
              <div className="mb-3 flex items-center gap-2">
                <span className="text-[11px] text-gray-400">Rows</span>
                <div className="flex overflow-hidden rounded border border-[var(--border)]">
                  {Array.from({ length: Math.min(n, 5) }, (_, i) => i + 1).map((r) => (
                    <button
                      key={r}
                      onClick={() => setRowTotal(r)}
                      className={`px-2 py-0.5 text-[11px] ${
                        counts.length === r
                          ? "bg-blue-600 text-white"
                          : "text-gray-300 hover:bg-[var(--border)]"
                      }`}
                    >
                      {r}
                    </button>
                  ))}
                </div>
              </div>

              {/* Presets. */}
              {presets(n).length > 0 ? (
                <div className="mb-3 flex flex-wrap gap-1">
                  {presets(n).map((p) => {
                    const label = p.join("·");
                    const active = p.join(",") === counts.join(",");
                    return (
                      <button
                        key={label}
                        onClick={() => setTileRows(p)}
                        className={`rounded border px-1.5 py-0.5 text-[11px] tabular-nums ${
                          active
                            ? "border-blue-500 bg-blue-600/20 text-blue-300"
                            : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)] hover:text-white"
                        }`}
                      >
                        {label}
                      </button>
                    );
                  })}
                </div>
              ) : null}

              {/* Free-text shape. */}
              <form
                className="mb-2 flex items-center gap-1.5"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (draftValid && parsed) setTileRows(parsed);
                }}
              >
                <input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  placeholder="3, 2"
                  className={`min-w-0 flex-1 rounded border bg-[var(--bg)] px-2 py-1 text-xs text-gray-200 outline-none ${
                    draft.trim() === "" || draftValid
                      ? "border-[var(--border)] focus:border-blue-500"
                      : "border-red-500/60"
                  }`}
                />
                <button
                  type="submit"
                  disabled={!draftValid}
                  className="rounded bg-blue-600 px-2 py-1 text-xs text-white hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Apply
                </button>
              </form>
              {draft.trim() !== "" && !draftValid ? (
                <p className="mb-2 text-[11px] text-red-400">
                  {parsed === null
                    ? "Use numbers separated by commas, e.g. 3, 2"
                    : `Adds up to ${draftTotal}, but there ${n === 1 ? "is" : "are"} ${n} tile${
                        n === 1 ? "" : "s"
                      }.`}
                </p>
              ) : null}

              <button
                onClick={resetTileLayout}
                className="mb-2 w-full rounded border border-[var(--border)] px-2 py-1 text-[11px] text-gray-300 hover:bg-[var(--border)] hover:text-white"
              >
                Reset to balanced
              </button>

              <div className="border-t border-[var(--border)] pt-2 text-[10.5px] leading-relaxed text-gray-500">
                <div>
                  <kbd className="text-gray-400">Alt</kbd> + arrows — move a tile
                </div>
                <div>
                  <kbd className="text-gray-400">Alt+Shift</kbd> + arrows — resize it
                </div>
                <div>
                  <kbd className="text-gray-400">Alt+F</kbd> — full-width row ·{" "}
                  <kbd className="text-gray-400">Alt+R</kbd> — reset
                </div>
              </div>
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
