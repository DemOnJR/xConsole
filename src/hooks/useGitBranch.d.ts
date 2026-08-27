import { type GitInfo } from "../lib/tauri";
/**
 * Debounced git status for a remote (SSH) or local path.
 * Clears immediately when the path changes so the UI never shows a stale branch.
 */
export declare function useGitBranch(opts: {
    /** When false, skip lookups (e.g. disconnected). */
    enabled: boolean;
    path: string | null | undefined;
    /** Remote VPS id — when set, uses SSH; when null, uses local filesystem. */
    vpsId?: string | null;
    debounceMs?: number;
}): GitInfo | null;
/** Compact git branch badge for panel headers (`main` or `main*` when dirty). */
export declare function GitBranchBadge({ info, className, }: {
    info: GitInfo | null;
    className?: string;
}): import("react").JSX.Element | null;
//# sourceMappingURL=useGitBranch.d.ts.map