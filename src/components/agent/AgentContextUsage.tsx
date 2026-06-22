import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import type { ContextUsage } from "../../lib/streamStats";
import { formatTokenCount } from "../../lib/streamStats";
import { ContextHourglass } from "./ContextHourglass";

const SEGMENT_COLORS: Record<string, string> = {
  system_prompt: "#6b7280",
  rules: "#22c55e",
  tool_definitions: "#a855f7",
  skills: "#f97316",
  memory: "#ec4899",
  infra: "#64748b",
  vps_prefetch: "#eab308",
  conversation_summary: "#f472b6",
  conversation: "#22d3ee",
};

const POPOVER_W = 280;

export function AgentContextUsageButton({
  usage,
  compactFlipCount,
  open,
  onToggle,
  onClose,
  placement = "composer",
}: {
  usage: ContextUsage | null;
  compactFlipCount: number;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
  /** Header opens popover downward; composer (above send) opens upward. */
  placement?: "header" | "composer";
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<{ top: number; left: number; above: boolean } | null>(
    null,
  );

  const pct = usage ? Math.round(usage.percent) : 0;

  const barSegments =
    usage?.segments.map((s) => ({
      ...s,
      widthPct: usage.total_tokens > 0 ? (s.tokens / usage.total_tokens) * 100 : 0,
      color: SEGMENT_COLORS[s.key] ?? "#475569",
    })) ?? [];

  const updateAnchor = () => {
    const btn = buttonRef.current;
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    const above = placement === "composer";
    setAnchor({
      top: above ? r.top - 8 : r.bottom + 8,
      left: Math.min(Math.max(8, r.right - POPOVER_W), window.innerWidth - POPOVER_W - 8),
      above,
    });
  };

  useEffect(() => {
    if (!open) {
      setAnchor(null);
      return;
    }
    updateAnchor();
    window.addEventListener("resize", updateAnchor);
    window.addEventListener("scroll", updateAnchor, true);
    return () => {
      window.removeEventListener("resize", updateAnchor);
      window.removeEventListener("scroll", updateAnchor, true);
    };
  }, [open, placement]);

  // Defer outside-click listener so the opening click does not instantly close the panel.
  useEffect(() => {
    if (!open) return;
    let remove: (() => void) | undefined;
    const timer = window.setTimeout(() => {
      const onDoc = (e: MouseEvent) => {
        const target = e.target as Node;
        if (rootRef.current?.contains(target)) return;
        if (popoverRef.current?.contains(target)) return;
        onClose();
      };
      document.addEventListener("mousedown", onDoc);
      remove = () => document.removeEventListener("mousedown", onDoc);
    }, 0);
    return () => {
      window.clearTimeout(timer);
      remove?.();
    };
  }, [open, onClose]);

  const popover =
    open && anchor
      ? createPortal(
          <div
            ref={popoverRef}
            className="fixed z-[9999] w-[280px] rounded-lg border border-[#1f2737] bg-[#0d121b] p-3 shadow-xl"
            style={{
              top: anchor.top,
              left: anchor.left,
              transform: anchor.above ? "translateY(-100%)" : undefined,
            }}
          >
            <div className="mb-2 flex items-start justify-between gap-2">
              <div>
                <div className="text-xs font-medium text-gray-200">Context Usage</div>
                {usage ? (
                  <div className="mt-0.5 text-[11px] text-gray-400">
                    {pct}% full · ~{formatTokenCount(usage.total_tokens)} /{" "}
                    {formatTokenCount(usage.context_limit)} tokens
                  </div>
                ) : (
                  <div className="mt-0.5 text-[11px] text-gray-500">
                    Send a message to measure context fill.
                  </div>
                )}
              </div>
              <button
                type="button"
                onClick={onClose}
                className="rounded p-0.5 text-gray-500 hover:bg-[#1f2737] hover:text-gray-300"
                aria-label="Close"
              >
                ✕
              </button>
            </div>

            {usage ? (
              <>
                <div className="mb-3 flex h-2 overflow-hidden rounded-full bg-[#1f2737]">
                  {barSegments.map((s) => (
                    <div
                      key={s.key}
                      style={{ width: `${s.widthPct}%`, backgroundColor: s.color }}
                      title={`${s.label}: ${formatTokenCount(s.tokens)}`}
                    />
                  ))}
                </div>

                <ul className="max-h-56 space-y-1.5 overflow-y-auto">
                  {usage.segments.map((s) => (
                    <li
                      key={s.key}
                      className="flex items-center justify-between gap-2 text-[11px]"
                    >
                      <span className="flex min-w-0 items-center gap-1.5 text-gray-300">
                        <span
                          className="h-2 w-2 shrink-0 rounded-sm"
                          style={{ backgroundColor: SEGMENT_COLORS[s.key] ?? "#475569" }}
                        />
                        <span className="truncate">{s.label}</span>
                      </span>
                      <span className="shrink-0 font-mono tabular-nums text-gray-500">
                        ~{formatTokenCount(s.tokens)}
                      </span>
                    </li>
                  ))}
                </ul>

                <p className="mt-2 text-[10px] leading-relaxed text-gray-600">
                  Sand drains as context fills. Auto-compact flips the hourglass and frees space.
                </p>
              </>
            ) : (
              <p className="text-[11px] leading-relaxed text-gray-500">
                Context breakdown appears after the agent starts a turn and reports token usage.
              </p>
            )}
          </div>,
          document.body,
        )
      : null;

  return (
    <div className="relative" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        onClick={onToggle}
        className={`flex flex-col items-center gap-1 rounded-md transition-colors ${
          placement === "composer"
            ? `px-1.5 py-1 hover:text-gray-200 ${open ? "text-gray-100" : "text-gray-400"}`
            : `px-1.5 py-1 font-mono text-[10px] tabular-nums hover:bg-[#1f2737] hover:text-gray-200 ${
                open ? "bg-[#1f2737] text-gray-100" : "text-gray-400"
              }`
        }`}
        title={
          usage
            ? `Context ${pct}% full — click for breakdown`
            : "Context usage — click for breakdown"
        }
        aria-expanded={open}
        aria-haspopup="dialog"
      >
        <ContextHourglass
          percent={pct}
          compactFlipCount={compactFlipCount}
          size={placement === "composer" ? 26 : 16}
        />
        {placement === "composer" ? (
          <span className="font-mono text-[10px] tabular-nums leading-none">{pct}%</span>
        ) : (
          <span>{pct}% ctx</span>
        )}
      </button>
      {popover}
    </div>
  );
}
