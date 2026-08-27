import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/tauri";
/**
 * Debounced git status for a remote (SSH) or local path.
 * Clears immediately when the path changes so the UI never shows a stale branch.
 */
export function useGitBranch(opts) {
    const { enabled, path, vpsId, debounceMs = 350 } = opts;
    const [info, setInfo] = useState(null);
    const gen = useRef(0);
    useEffect(() => {
        const id = ++gen.current;
        setInfo(null);
        if (!enabled || !path?.trim())
            return;
        const t = window.setTimeout(() => {
            const run = async () => {
                try {
                    const b = vpsId
                        ? await api.remoteGitBranch(vpsId, path)
                        : await api.localGitBranch(path);
                    if (gen.current === id)
                        setInfo(b);
                }
                catch {
                    if (gen.current === id)
                        setInfo(null);
                }
            };
            void run();
        }, debounceMs);
        return () => clearTimeout(t);
    }, [enabled, path, vpsId, debounceMs]);
    return info;
}
/** Compact git branch badge for panel headers (`main` or `main*` when dirty). */
export function GitBranchBadge({ info, className = "", }) {
    if (!info?.branch)
        return null;
    const label = info.dirty ? `${info.branch}*` : info.branch;
    const tipParts = [
        `git · ${info.branch}`,
        info.dirty ? "uncommitted changes" : null,
        info.root ? `root: ${info.root}` : null,
    ].filter(Boolean);
    const tip = tipParts.join(" · ");
    return (_jsxs("span", { className: `inline-flex max-w-[160px] items-center gap-1 truncate rounded px-1.5 py-0.5 font-mono text-[10px] ${info.dirty
            ? "bg-amber-950/55 text-amber-300/95"
            : "bg-emerald-950/50 text-emerald-300/95"} ${className}`, "data-tooltip": tip, title: tip, onDoubleClick: (e) => {
            e.stopPropagation();
            // Quick copy branch name for scripts / PRs.
            void navigator.clipboard?.writeText(info.branch).catch(() => { });
        }, children: [_jsx("span", { className: "opacity-70", "aria-hidden": true, children: "\u2387" }), _jsx("span", { className: "truncate", children: label })] }));
}
//# sourceMappingURL=useGitBranch.js.map