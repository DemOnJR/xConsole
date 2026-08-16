export interface DiffLine {
  kind: "add" | "del" | "same";
  text: string;
  oldLineNumber?: number;
  newLineNumber?: number;
}

export interface DiffResult {
  lines: DiffLine[];
  addedCount: number;
  removedCount: number;
  hasChanges: boolean;
}

/**
 * Compute line-by-line diff between two plan texts (LCS based).
 * Returns array of DiffLine with line numbers and added/deleted counts.
 */
export function computePlanDiff(oldText: string, newText: string): DiffResult {
  if (!oldText && !newText) {
    return { lines: [], addedCount: 0, removedCount: 0, hasChanges: false };
  }

  const oldLines = oldText ? oldText.split("\n") : [];
  const newLines = newText ? newText.split("\n") : [];

  const m = oldLines.length;
  const n = newLines.length;

  // LCS dynamic programming table
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));

  for (let i = 0; i < m; i++) {
    for (let j = 0; j < n; j++) {
      if (oldLines[i] === newLines[j]) {
        dp[i + 1][j + 1] = dp[i][j] + 1;
      } else {
        dp[i + 1][j + 1] = Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
  }

  const lines: DiffLine[] = [];
  let i = m;
  let j = n;
  let addedCount = 0;
  let removedCount = 0;

  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
      lines.unshift({
        kind: "same",
        text: oldLines[i - 1],
        oldLineNumber: i,
        newLineNumber: j,
      });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      lines.unshift({
        kind: "add",
        text: newLines[j - 1],
        newLineNumber: j,
      });
      addedCount++;
      j--;
    } else if (i > 0 && (j === 0 || dp[i][j - 1] < dp[i - 1][j])) {
      lines.unshift({
        kind: "del",
        text: oldLines[i - 1],
        oldLineNumber: i,
      });
      removedCount++;
      i--;
    }
  }

  return {
    lines,
    addedCount,
    removedCount,
    hasChanges: addedCount > 0 || removedCount > 0,
  };
}
