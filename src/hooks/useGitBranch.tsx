import { useEffect, useRef, useState } from "react";
import { api } from "../lib/tauri";

/**
 * Debounced git branch for a remote (SSH) or local path.
 * Clears immediately when the path changes so the UI never shows a stale branch.
 */
export function useGitBranch(opts: {
  /** When false, skip lookups (e.g. disconnected). */
  enabled: boolean;
  path: string | null | undefined;
  /** Remote VPS id — when set, uses SSH; when null, uses local filesystem. */
  vpsId?: string | null;
  debounceMs?: number;
}): string | null {
  const { enabled, path, vpsId, debounceMs = 350 } = opts;
  const [branch, setBranch] = useState<string | null>(null);
  const gen = useRef(0);

  useEffect(() => {
    const id = ++gen.current;
    setBranch(null);
    if (!enabled || !path?.trim()) return;

    const t = window.setTimeout(() => {
      const run = async () => {
        try {
          const b = vpsId
            ? await api.remoteGitBranch(vpsId, path)
            : await api.localGitBranch(path);
          if (gen.current === id) setBranch(b);
        } catch {
          if (gen.current === id) setBranch(null);
        }
      };
      void run();
    }, debounceMs);

    return () => clearTimeout(t);
  }, [enabled, path, vpsId, debounceMs]);

  return branch;
}

/** Compact git branch badge for panel headers. */
export function GitBranchBadge({
  branch,
  className = "",
}: {
  branch: string | null;
  className?: string;
}) {
  if (!branch) return null;
  return (
    <span
      className={`inline-flex max-w-[140px] items-center gap-1 truncate rounded bg-emerald-950/50 px-1.5 py-0.5 font-mono text-[10px] text-emerald-300/95 ${className}`}
      data-tooltip={`git · ${branch}`}
      title={`git · ${branch}`}
    >
      <span className="opacity-70" aria-hidden>
        ⎇
      </span>
      <span className="truncate">{branch}</span>
    </span>
  );
}
