import { useEffect, useRef, useState } from "react";
import { useCanvasStore } from "../stores/canvasStore";
import {
  columnCounts,
  defaultRowCounts,
  parseColumnCounts,
  parseRowCounts,
  reconcile,
  rowCounts,
  type TileLayout,
} from "../lib/tileLayout";
import { ColumnsIcon, RowsIcon } from "./icons";

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

/** Column presets for `n` tiles: side-by-side panes, extras stacked left. */
function columnPresets(n: number): number[][] {
  if (n <= 1) return [];
  const out: number[][] = [];
  const add = (counts: number[]) => {
    const usable = counts.filter((c) => c > 0);
    if (usable.reduce((a, b) => a + b, 0) !== n) return;
    if (out.some((o) => o.join(",") === usable.join(","))) return;
    out.push(usable);
  };
  // Two columns first: the "sidebar" look the user wants.
  add([Math.ceil(n / 2), Math.floor(n / 2)]);
  add([1, n - 1]); // one narrow left, rest stacked right
  add([n - 1, 1]); // rest stacked left, one narrow right
  // Even splits into 3..4 columns, extras on the left.
  for (let cols = 3; cols <= Math.min(4, n); cols++) {
    const base = Math.floor(n / cols);
    const extra = n % cols;
    if (base === 0) continue;
    add(Array.from({ length: cols }, (_, c) => base + (c < extra ? 1 : 0)));
  }
  return out.slice(0, 6);
}

/**
 * The tile-shape editor: pick how many tiles sit in each row, or each column.
 *
 * Rows are the whole model — a row always fills the full width and splits it between
 * the tiles it holds, so "3 on top, 2 on the bottom, both full width" is the shape
 * `3, 2`, and a row holding one tile spans the width on its own. Switching to
 * **Columns** tiles the same windows side-by-side instead: `2, 1` is two stacked on
 * the left and one full-height on the right — a sidebar arrangement.
 */
export function TileLayoutMenu() {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const wrapRef = useRef<HTMLDivElement | null>(null);

  const nodes = useCanvasStore((s) => s.nodes);
  const tileLayout = useCanvasStore((s) => s.tileLayout);
  const setTileRows = useCanvasStore((s) => s.setTileRows);
  const setTileColumns = useCanvasStore((s) => s.setTileColumns);
  const resetTileLayout = useCanvasStore((s) => s.resetTileLayout);

  const n = nodes.length;
  // Reconcile before reading the shape: the stored layout can lag the node list by a
  // render (or come from a previous session) and would otherwise show a stale count.
  const layout: TileLayout = reconcile(tileLayout, nodes.map((node) => node.id));
  const isColumns = !!layout.columns;
  const counts = isColumns ? columnCounts(layout) : rowCounts(layout);
  const shape = counts.join(isColumns ? " | " : ", ");

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

  const parsed = isColumns ? parseColumnCounts(draft) : parseRowCounts(draft);
  const draftTotal = parsed?.reduce((a, b) => a + b, 0) ?? 0;
  const draftValid = parsed !== null && draftTotal === n;

  /** Move one tile between two rows/columns, keeping the total constant. */
  const bump = (index: number, delta: number) => {
    const next = [...counts];
    const donor = delta > 0 ? next.findIndex((c, i) => i !== index && c > 1) : -1;
    if (delta > 0) {
      // Pull a tile in from another row/column so the total never changes.
      if (donor === -1) return;
      next[donor] -= 1;
      next[index] += 1;
    } else {
      if (next[index] <= 1) return;
      next[index] -= 1;
      // Push it to the next row/column down, or start a new one.
      if (index + 1 < next.length) next[index + 1] += 1;
      else next.push(1);
    }
    const cleaned = next.filter((c) => c > 0);
    if (isColumns) setTileColumns(cleaned);
    else setTileRows(cleaned);
  };

  const setTotal = (count: number) => {
    if (count < 1 || count > n) return;
    const base = Math.floor(n / count);
    const extra = n % count;
    const shape = Array.from({ length: count }, (_, i) => base + (i < extra ? 1 : 0));
    if (isColumns) setTileColumns(shape);
    else setTileRows(shape);
  };

  return (
    <div className="relative" ref={wrapRef}>
      <button
        data-tooltip="Tile shape — how many terminals per row or column"
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
              {/* Rows / Columns mode toggle. */}
              <div className="mb-2 flex items-center gap-1">
                <button
                  onClick={() => {
                    if (!isColumns) return;
                    // Switch from columns back to rows: rows already mirrors the
                    // same node order, so just drop the column view.
                    useCanvasStore.getState().setTileLayout({
                      rows: layout.rows,
                    });
                  }}
                  className={`flex flex-1 items-center justify-center gap-1.5 rounded border px-2 py-1 text-[11px] transition ${
                    !isColumns
                      ? "border-blue-500 bg-blue-600/20 text-blue-300"
                      : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)] hover:text-white"
                  }`}
                  data-tooltip="Stack in horizontal rows"
                >
                  <RowsIcon size={13} /> Rows
                </button>
                <button
                  onClick={() => {
                    if (isColumns) return;
                    // Switch from rows to columns: split the current order into two
                    // balanced side-by-side panes (extras stacked left).
                    const n = nodes.length;
                    const counts = [Math.ceil(n / 2), Math.floor(n / 2)];
                    setTileColumns(counts);
                  }}
                  className={`flex flex-1 items-center justify-center gap-1.5 rounded border px-2 py-1 text-[11px] transition ${
                    isColumns
                      ? "border-blue-500 bg-blue-600/20 text-blue-300"
                      : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)] hover:text-white"
                  }`}
                  data-tooltip="Stack in side-by-side columns (like sidebars)"
                >
                  <ColumnsIcon size={13} /> Columns
                </button>
              </div>

              <div className="mb-2 flex items-baseline justify-between">
                <span className="text-xs font-medium text-gray-200">
                  {isColumns ? "Tiles per column" : "Tiles per row"}
                </span>
                <span className="text-[11px] text-gray-500">
                  {n} tile{n === 1 ? "" : "s"}
                </span>
              </div>

              {/* Live preview of the shape. */}
              <div
                className={`mb-3 flex rounded border border-[var(--border)] bg-[var(--bg)] p-1.5 ${
                  isColumns ? "gap-[3px]" : "flex-col gap-[3px]"
                }`}
              >
                {counts.map((c, r) =>
                  isColumns ? (
                    <div key={r} className="flex flex-col gap-[3px]" style={{ width: 14 }}>
                      {Array.from({ length: c }, (_, i) => (
                        <div key={i} className="flex-1 rounded-[2px] bg-blue-600/40" />
                      ))}
                    </div>
                  ) : (
                    <div key={r} className="flex gap-[3px]" style={{ height: 14 }}>
                      {Array.from({ length: c }, (_, i) => (
                        <div key={i} className="flex-1 rounded-[2px] bg-blue-600/40" />
                      ))}
                    </div>
                  ),
                )}
              </div>

              {/* Per-row/column steppers. */}
              <div className="mb-3 flex flex-col gap-1.5">
                {counts.map((c, r) => (
                  <div key={r} className="flex items-center gap-2 text-[11px] text-gray-400">
                    <span className="w-10 shrink-0">
                      {isColumns ? `Col ${r + 1}` : `Row ${r + 1}`}
                    </span>
                    <button
                      className={STEP_BTN}
                      disabled={c <= 1}
                      onClick={() => bump(r, -1)}
                      data-tooltip={isColumns ? "One fewer tile in this column" : "One fewer tile in this row"}
                    >
                      −
                    </button>
                    <span className="w-4 text-center tabular-nums text-gray-200">{c}</span>
                    <button
                      className={STEP_BTN}
                      disabled={!counts.some((other, i) => i !== r && other > 1)}
                      onClick={() => bump(r, 1)}
                      data-tooltip={isColumns ? "One more tile in this column" : "One more tile in this row"}
                    >
                      +
                    </button>
                  </div>
                ))}
              </div>

              {/* Count shortcut. */}
              <div className="mb-3 flex items-center gap-2">
                <span className="text-[11px] text-gray-400">
                  {isColumns ? "Columns" : "Rows"}
                </span>
                <div className="flex overflow-hidden rounded border border-[var(--border)]">
                  {Array.from({ length: Math.min(n, 5) }, (_, i) => i + 1).map((r) => (
                    <button
                      key={r}
                      onClick={() => setTotal(r)}
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
              {(isColumns ? columnPresets(n) : presets(n)).length > 0 ? (
                <div className="mb-3 flex flex-wrap gap-1">
                  {(isColumns ? columnPresets(n) : presets(n)).map((p) => {
                    const label = p.join("·");
                    const active = p.join(",") === counts.join(",");
                    return (
                      <button
                        key={label}
                        onClick={() => (isColumns ? setTileColumns(p) : setTileRows(p))}
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
                  if (draftValid && parsed) {
                    if (isColumns) setTileColumns(parsed);
                    else setTileRows(parsed);
                  }
                }}
              >
                <input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  placeholder={isColumns ? "2 | 1" : "3, 2"}
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
                    ? isColumns
                      ? "Use numbers separated by |, e.g. 2 | 1"
                      : "Use numbers separated by commas, e.g. 3, 2"
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
