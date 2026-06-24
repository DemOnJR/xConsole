// A small line-level diff (LCS) for the changes panel. Produces unified rows for a
// GitHub-style view plus added/removed counts. No dependency; good enough for the
// typical agent-edited file. Very large inputs fall back to a coarse block diff so
// we never blow up on an O(n·m) table.

export type DiffRowType = "ctx" | "add" | "del";

export interface DiffRow {
  type: DiffRowType;
  text: string;
  oldNo?: number;
  newNo?: number;
}

export interface DiffResult {
  rows: DiffRow[];
  added: number;
  removed: number;
}

const MAX_CELLS = 2_000_000; // LCS table cap (~1400×1400 lines)

function splitLines(s: string): string[] {
  if (s.length === 0) return [];
  // Normalize CRLF so Windows/Unix content compares cleanly.
  return s.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
}

export function lineDiff(before: string, after: string): DiffResult {
  const a = splitLines(before);
  const b = splitLines(after);
  const n = a.length;
  const m = b.length;

  if (n === 0 && m === 0) return { rows: [], added: 0, removed: 0 };

  // Coarse fallback for pathologically large inputs.
  if (n * m > MAX_CELLS) {
    const rows: DiffRow[] = [];
    a.forEach((text, i) => rows.push({ type: "del", text, oldNo: i + 1 }));
    b.forEach((text, i) => rows.push({ type: "add", text, newNo: i + 1 }));
    return { rows, added: m, removed: n };
  }

  // LCS length table.
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const rows: DiffRow[] = [];
  let added = 0;
  let removed = 0;
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      rows.push({ type: "ctx", text: a[i], oldNo: i + 1, newNo: j + 1 });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      rows.push({ type: "del", text: a[i], oldNo: i + 1 });
      removed++;
      i++;
    } else {
      rows.push({ type: "add", text: b[j], newNo: j + 1 });
      added++;
      j++;
    }
  }
  while (i < n) {
    rows.push({ type: "del", text: a[i], oldNo: i + 1 });
    removed++;
    i++;
  }
  while (j < m) {
    rows.push({ type: "add", text: b[j], newNo: j + 1 });
    added++;
    j++;
  }

  return { rows, added, removed };
}
