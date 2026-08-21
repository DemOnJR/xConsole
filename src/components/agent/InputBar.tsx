import { memo, useMemo, type MouseEvent, type PointerEvent } from "react";
import type { AiProvider } from "../../lib/tauri";
import type { ContextUsage, SessionCacheTotals, TokenStats } from "../../lib/streamStats";
import { BrainIcon, EyeIcon, PlanIcon, ServerIcon, ShieldIcon, SparkIcon } from "../icons";
import { ContextGauge } from "./ContextGauge";
import { CacheMeter } from "./AgentTokenStats";
import { useMaskHost } from "../../lib/privacy";

export interface TargetServerInfo {
  id: string;
  name: string;
  host: string;
}

export type ReasoningLevel = "off" | "low" | "medium" | "high";

/** Does this provider+model support a reasoning-effort control? */
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

/** Composer footer: targets · provider·model · reasoning · plan · permissions ·
 *  ctx gauge · cost · git branch · send/stop. */
export const InputBar = memo(function InputBar({
  activeTargets,
  onPickTargets,
  activeProvider,
  activeModel,
  reasoning,
  onReasoning,
  planMode,
  onTogglePlan,
  agentMode,
  onPickMode,
  safetyMode,
  onCycleSafety,
  onPickSafety,
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
  onPickReasoning,
}: {
  activeTargets?: TargetServerInfo[];
  onPickTargets?: () => void;
  activeProvider?: AiProvider;
  activeModel?: string;
  reasoning: ReasoningLevel;
  onReasoning: (r: ReasoningLevel) => void;
  planMode: boolean;
  onTogglePlan: () => void;
  agentMode?: import("../../stores/agentStore").AgentRuntimeMode;
  onPickMode?: () => void;
  safetyMode: string;
  onCycleSafety?: () => void;
  onPickSafety?: () => void;
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
  onPickReasoning?: () => void;
}) {
  const model = activeModel || activeProvider?.model;
  const canReason = reasoningCapable(activeProvider?.kind, model ?? undefined);
  const stopNode = (e: PointerEvent | MouseEvent) => e.stopPropagation();
  const modelTip = [activeProvider?.name ?? "no provider", model].filter(Boolean).join(" · ");
  const safety = safetyMode || "approve";
  const safetyTone =
    safety === "full"
      ? "text-emerald-300"
      : safety === "allowlist"
        ? "text-amber-300"
        : "text-red-300";
  const cycleReasoning = () => {
    const order: ReasoningLevel[] = ["off", "low", "medium", "high"];
    const i = order.indexOf(reasoning);
    onReasoning(order[(i + 1) % order.length] ?? "off");
  };
  const maskHost = useMaskHost();
  const targetsTip = useMemo(() => {
    if (!activeTargets || activeTargets.length === 0) {
      return "Targets: None active\nSelect target servers (/targets)";
    }
    const countStr = `Targets (${activeTargets.length} active):`;
    const listStr = activeTargets.map((t) => `• ${t.name} (${maskHost(t.host)})`).join("\n");
    return `${countStr}\n${listStr}\nManage targets (/targets)`;
  }, [activeTargets, maskHost]);
  const iconBtn =
    "flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--text-dim)] transition hover:bg-[var(--border)]/50 hover:text-[var(--text)]";

  return (
    <div className="xc-input-bar flex select-none flex-wrap items-center gap-0.5 border-t border-[var(--border)]/60 px-1.5 pb-1.5 pt-1">
      {onPickTargets ? (
        <button
          type="button"
          className={`${iconBtn} ${activeTargets && activeTargets.length > 0 ? "text-emerald-400" : ""}`}
          onClick={onPickTargets}
          onPointerDown={stopNode}
          onMouseDown={stopNode}
          aria-label="Target servers"
          data-tooltip={targetsTip}
        >
          <ServerIcon size={14} />
        </button>
      ) : null}

      <button
        type="button"
        className={iconBtn}
        onClick={onPickModel}
        onPointerDown={stopNode}
        onMouseDown={stopNode}
        aria-label="Switch model"
        data-tooltip={`${modelTip}\nSwitch provider/model (/model)`}
      >
        <SparkIcon size={14} />
      </button>

      <button
        type="button"
        className={`${iconBtn} ${safetyTone}`}
        onClick={onPickSafety ?? onCycleSafety}
        onPointerDown={stopNode}
        onMouseDown={stopNode}
        aria-label={`Safety profile ${safety}`}
        data-tooltip={`Safety: ${safety}\nChoose permission level (/safety)`}
      >
        <ShieldIcon size={14} />
      </button>

      <button
        type="button"
        className={`${iconBtn} text-[10px] font-mono font-medium px-2 py-0.5 flex items-center gap-1 rounded transition ${
          agentMode === "code"
            ? "bg-amber-500/20 text-amber-300 border border-amber-500/30"
            : agentMode === "plan" || planMode
              ? "bg-indigo-500/20 text-indigo-300 border border-indigo-500/30"
              : agentMode === "minimal"
                ? "bg-purple-500/20 text-purple-300 border border-purple-500/30"
                : agentMode === "standard"
                  ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/30"
                  : "bg-cyan-500/20 text-cyan-300 border border-cyan-500/30"
        }`}
        onClick={onPickMode ?? onTogglePlan}
        onPointerDown={stopNode}
        onMouseDown={stopNode}
        aria-label={`Mode: ${agentMode || (planMode ? "plan" : "auto")}`}
        data-tooltip={`Runtime Mode: ${agentMode || (planMode ? "plan" : "auto")}\n📋 Plan · 🌐 Standard (Std) · ⚡ Code · 🤖 Auto · 🛡️ Minimal (/mode · Shift+Tab)`}
      >
        <PlanIcon size={14} className="shrink-0" />
        <span className="font-semibold">
          {agentMode === "code" ? (
            "code"
          ) : agentMode === "plan" || planMode ? (
            "plan"
          ) : agentMode === "minimal" ? (
            "min"
          ) : agentMode === "standard" ? (
            "std"
          ) : (
            "auto"
          )}
        </span>
      </button>

      {onPickVision ? (
        <button
          type="button"
          className={iconBtn}
          onClick={onPickVision}
          onPointerDown={stopNode}
          onMouseDown={stopNode}
          aria-label="Vision model"
          data-tooltip={`${visionLabel || "vision"}\nImage vision — screenshots (/vision)`}
        >
          <EyeIcon size={14} />
        </button>
      ) : null}

      {canReason ? (
        <button
          type="button"
          className={`${iconBtn} ${reasoning !== "off" ? "text-violet-300" : ""}`}
          onClick={onPickReasoning ?? cycleReasoning}
          onPointerDown={stopNode}
          onMouseDown={stopNode}
          aria-label={`Reasoning ${reasoning}`}
          data-tooltip={`Reasoning: ${reasoning}\nChoose reasoning effort (/reasoning)`}
        >
          <BrainIcon size={14} />
        </button>
      ) : null}

      <div className="ml-auto flex min-w-0 items-center gap-0.5">
        {gitLabel ? (
          <span
            className="xc-git-pill max-w-[88px] truncate px-1 text-[10px] text-[var(--text-faint)]"
            data-tooltip={`Repo: ${gitLabel}`}
          >
            ⎇ {gitLabel}
          </span>
        ) : null}

        <CacheMeter
          stats={streamStats}
          sessionCache={sessionCache}
          costUsd={costUsd}
          onClick={onPickContext}
        />

        <ContextGauge
          usage={contextUsage}
          onClick={onPickContext}
          onPointerDown={stopNode}
        />

        {streaming ? (
          <>
            <button
              type="button"
              onClick={onSend}
              onPointerDown={stopNode}
              onMouseDown={stopNode}
              aria-label="Queue follow-up"
              data-tooltip="Queue this message — you can edit it before it sends"
              className={iconBtn}
            >
              <span className="block h-0 w-0 border-y-[4px] border-l-[6px] border-y-transparent border-l-current" />
            </button>
            <button
              type="button"
              onClick={onStop}
              onPointerDown={(e) => {
                stopNode(e);
                onStop();
              }}
              onMouseDown={(e) => {
                stopNode(e);
              }}
              aria-label="Stop generation"
              data-tooltip="Stop generation"
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-red-600/90 text-white transition hover:bg-red-600 active:scale-95"
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
            data-tooltip="Send"
            className="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-blue-600 text-white transition hover:bg-blue-500"
          >
            <span className="block h-0 w-0 border-y-[4px] border-l-[6px] border-y-transparent border-l-white" />
          </button>
        )}
      </div>
    </div>
  );
});
