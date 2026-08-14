import type { MouseEvent, PointerEvent } from "react";
import type { AiProvider } from "../../lib/tauri";
import type { ContextUsage, SessionCacheTotals, TokenStats } from "../../lib/streamStats";
import { ContextGauge } from "./ContextGauge";
import { CacheMeter } from "./AgentTokenStats";

export type ReasoningLevel = "off" | "low" | "medium" | "high";

/** Does this provider+model support a reasoning-effort control? (t3code traits.) */
export function reasoningCapable(kind: string | undefined, _model: string | undefined): boolean {
  const k = (kind ?? "").toLowerCase();
  if (k === "anthropic") return true; // thinking budgets on Sonnet/Opus
  if (k === "openai" || k === "ollama") {
    // OpenAI reasoning models + Ollama think; conservative default for openai.
    if (k === "ollama") return true;
    return true; // openai-compat: effort is harmless
  }
  return false;
}

/** t3code-style composer footer: provider·model · reasoning · plan · permissions ·
 *  ctx gauge · cost · git branch · send/stop. */
export function InputBar({
  activeProvider,
  activeModel,
  reasoning,
  onReasoning,
  planMode,
  onTogglePlan,
  safetyMode,
  onCycleSafety,
  contextUsage,
  streamStats,
  sessionCache,
  costUsd,
  gitLabel,
  streaming,
  onSend,
  onStop,
  onPickModel,
  onPickContext,
  visionLabel,
  onPickVision,
}: {
  activeProvider?: AiProvider;
  activeModel?: string;
  reasoning: ReasoningLevel;
  onReasoning: (r: ReasoningLevel) => void;
  planMode: boolean;
  onTogglePlan: () => void;
  safetyMode: string;
  onCycleSafety: () => void;
  contextUsage: ContextUsage | null;
  streamStats: TokenStats | null;
  sessionCache?: SessionCacheTotals | null;
  costUsd: number;
  gitLabel: string | null;
  streaming: boolean;
  onSend: () => void;
  onStop: () => void;
  onPickModel: () => void;
  onPickContext: () => void;
  visionLabel?: string;
  onPickVision?: () => void;
}) {
  const model = activeModel || activeProvider?.model;
  const canReason = reasoningCapable(activeProvider?.kind, model ?? undefined);
  const stopNode = (e: PointerEvent | MouseEvent) => e.stopPropagation();

  const pill =
    "flex items-center gap-1 rounded border border-[var(--border)] px-1.5 py-0.5 text-[10px] text-[var(--text-dim)] hover:text-[var(--text)] hover:bg-[var(--border)]/40 transition";

  return (
    <div className="flex select-none flex-wrap items-center gap-1.5 border-t border-[var(--border)]/60 px-2 pb-2 pt-1.5">
      {/* Provider · model */}
      <button
        type="button"
        className={pill}
        onClick={onPickModel}
        onPointerDown={stopNode}
        onMouseDown={stopNode}
        data-tooltip="Switch provider/model (/model)"
      >
        <span className="max-w-[120px] truncate text-gray-300">
          {activeProvider?.name ?? "no provider"}
        </span>
        {model ? <span className="max-w-[140px] truncate text-[var(--text-faint)]">· {model}</span> : null}
      </button>

      {/* Reasoning / effort (capability-driven) */}
      {canReason && (
        <select
          value={reasoning}
          onChange={(e) => onReasoning(e.target.value as ReasoningLevel)}
          data-tooltip="Reasoning effort"
          className="rounded border border-[var(--border)] bg-[var(--surface)] px-1 py-0.5 text-[10px] text-[var(--text-dim)] outline-none"
        >
          <option value="off">off</option>
          <option value="low">low</option>
          <option value="medium">medium</option>
          <option value="high">high</option>
        </select>
      )}

      {/* Plan toggle */}
      <button
        type="button"
        className={`${pill} ${planMode ? "border-indigo-500/50 bg-indigo-500/20 text-indigo-300" : ""}`}
        onClick={onTogglePlan}
        data-tooltip="Plan mode (Shift+Tab)"
      >
        plan
      </button>

      {onPickVision && (
        <button
          type="button"
          className={pill}
          onClick={onPickVision}
          onPointerDown={stopNode}
          onMouseDown={stopNode}
          data-tooltip="Image vision — model used to look at screenshots (/vision)"
        >
          <span className="max-w-[180px] truncate">{visionLabel || "vision"}</span>
        </button>
      )}

      {/* Permissions (safety mode) */}
      <button
        type="button"
        className={pill}
        onClick={onCycleSafety}
        data-tooltip="Safety mode — click to cycle (full / allowlist / approve)"
      >
        <span
          className={
            safetyMode === "full"
              ? "text-emerald-300"
              : safetyMode === "allowlist"
                ? "text-amber-300"
                : "text-red-300"
          }
        >
          {safetyMode || "approve"}
        </span>
      </button>

      <div className="ml-auto flex items-center gap-1.5">
        {/* Git branch */}
        {gitLabel && (
          <span className="max-w-[140px] truncate rounded border border-[var(--border)] px-1.5 py-0.5 text-[10px] text-[var(--text-faint)]" data-tooltip="Project repo">
            ⎇ {gitLabel}
          </span>
        )}

        <CacheMeter stats={streamStats} sessionCache={sessionCache} costUsd={costUsd} />

        {/* Context gauge — opens the /ctx breakdown, not the model list */}
        <ContextGauge
          usage={contextUsage}
          onClick={onPickContext}
          onPointerDown={stopNode}
        />

        {/* Send / Stop — while running, Send queues a follow-up */}
        {streaming ? (
          <>
            <button
              type="button"
              onClick={onSend}
              onPointerDown={stopNode}
              onMouseDown={stopNode}
              aria-label="Queue follow-up"
              data-tooltip="Queue this message — you can edit it before it sends"
              className="flex h-6 w-6 items-center justify-center rounded border border-[var(--border)] text-[var(--text-dim)] transition hover:bg-[var(--border)] hover:text-[var(--text)]"
            >
              <span className="block h-0 w-0 border-y-[4px] border-l-[6px] border-y-transparent border-l-current" />
            </button>
            <button
              type="button"
              onClick={onStop}
              onPointerDown={stopNode}
              onMouseDown={stopNode}
              aria-label="Stop generation"
              className="flex h-6 w-6 items-center justify-center rounded bg-red-600/90 text-white transition hover:bg-red-600"
            >
              <span className="block h-2 w-2 rounded-[2px] bg-white" />
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={onSend}
            onPointerDown={stopNode}
            onMouseDown={stopNode}
            aria-label="Send"
            className="flex h-6 w-6 items-center justify-center rounded bg-blue-600 text-white transition hover:bg-blue-500"
          >
            <span className="block h-0 w-0 border-y-[4px] border-l-[6px] border-y-transparent border-l-white" />
          </button>
        )}
      </div>
    </div>
  );
}
