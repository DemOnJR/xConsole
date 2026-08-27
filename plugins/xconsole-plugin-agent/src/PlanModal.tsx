import { useEffect, useMemo, useState } from "react";
import { useAgentStore } from "../../../src/stores/agentStore";
import { api, type AgentPlanFull } from "../../../src/lib/tauri";
import { computePlanDiff } from "../../../src/lib/planDiff";
import { StopIcon } from "../../../src/components/icons";

const STATUS_BADGE: Record<string, { label: string; cls: string }> = {
  presented: { label: "Awaiting review", cls: "text-amber-300 border-amber-500/40 bg-amber-500/10" },
  applied: { label: "Applied", cls: "text-emerald-300 border-emerald-500/40 bg-emerald-500/10" },
  archived: { label: "Archived", cls: "text-gray-400 border-gray-500/40 bg-gray-500/10" },
  cancelled: { label: "Cancelled", cls: "text-red-300 border-red-500/40 bg-red-500/10" },
};

/** Parse a SQLite `datetime('now')` value (UTC, no zone) defensively. */
function formatTimestamp(s: string): string {
  // "YYYY-MM-DD HH:MM:SS" — treat as UTC so WebKit doesn't produce Invalid Date.
  const iso = /^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}/.test(s) ? `${s.replace(" ", "T")}Z` : s;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return s;
  return d.toLocaleString();
}

/**
 * Full-window plan review modal, rendered above the terminal canvas.
 * The plan is editable; revisions are sent back to the agent as feedback and
 * the agent re-presents through `present_plan` (new ai://plan event swaps it).
 */
export function PlanModal() {
  const pendingPlan = useAgentStore((s) => s.pendingPlan);
  const previousPlanText = useAgentStore((s) => s.previousPlanText);
  const planDraft = useAgentStore((s) => s.planDraft);
  const planHistory = useAgentStore((s) => s.planHistory);
  const planHistoryOpen = useAgentStore((s) => s.planHistoryOpen);
  const planRevising = useAgentStore((s) => s.planRevising);
  const streamingText = useAgentStore((s) => s.streamingText);
  const activity = useAgentStore((s) => s.activity);
  const setPlanDraft = useAgentStore((s) => s.setPlanDraft);
  const applyPlan = useAgentStore((s) => s.applyPlan);
  const archivePlanAction = useAgentStore((s) => s.archivePlanAction);
  const cancelPlanAction = useAgentStore((s) => s.cancelPlanAction);
  const revisePlan = useAgentStore((s) => s.revisePlan);
  const stopPlanRevision = useAgentStore((s) => s.stopPlanRevision);
  const loadPlanHistory = useAgentStore((s) => s.loadPlanHistory);
  const setPlanHistoryOpen = useAgentStore((s) => s.setPlanHistoryOpen);
  const closePlanModal = useAgentStore((s) => s.closePlanModal);

  const [feedback, setFeedback] = useState("");
  const [viewing, setViewing] = useState<AgentPlanFull | null>(null);
  const [sending, setSending] = useState(false);
  const [viewMode, setViewMode] = useState<"edit" | "preview" | "diff">("edit");

  const open = !!pendingPlan || planHistoryOpen;

  const diff = useMemo(
    () => computePlanDiff(previousPlanText || "", planDraft || ""),
    [previousPlanText, planDraft],
  );

  // Automatically switch to diff view if a revision with diff changes arrives
  useEffect(() => {
    if (previousPlanText && diff.hasChanges) {
      setViewMode("diff");
    }
  }, [previousPlanText, diff.hasChanges]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      // Never steal Esc while the user is typing in an editor/input.
      const el = document.activeElement;
      if (el && (el.tagName === "TEXTAREA" || el.tagName === "INPUT")) return;
      if (e.key === "Escape") {
        e.preventDefault();
        if (viewing) {
          setViewing(null);
        } else {
          void closePlanModal();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, viewing, closePlanModal]);

  if (!open) return null;

  const sendRevision = async (mode: "feedback" | "apply-draft") => {
    if (!pendingPlan) return;
    setSending(true);
    try {
      if (mode === "apply-draft") {
        const msg =
          `Use this revised plan instead (user edited it by hand):\n\n${planDraft}`.trim();
        await revisePlan(pendingPlan.id, msg);
      } else {
        await revisePlan(pendingPlan.id, feedback);
        setFeedback("");
      }
    } catch (e) {
      // Surface failures instead of silently keeping the modal stuck.
      setFeedback((f) => f || String(e));
    } finally {
      setSending(false);
    }
  };

  const openHistory = async (id: string) => {
    const full = await api.getPlan(id).catch(() => null);
    setViewing(full);
  };

  const badge = (status: string) => STATUS_BADGE[status] ?? STATUS_BADGE.presented;

  const runningActivity = activity.find((a) => a.state === "running");
  const activeActivityLabel = runningActivity
    ? runningActivity.detail || runningActivity.label
    : null;
  const streamingSnippet = streamingText ? streamingText.trim().slice(-240) : "";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <div className="flex h-[min(85vh,820px)] w-[min(980px,94vw)] flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface-2)] shadow-[var(--shadow-panel)]">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-semibold text-[var(--text)]">
              {viewing
                ? `Plan: ${viewing.title ?? "Untitled"}`
                : pendingPlan
                  ? `Plan: ${pendingPlan.title ?? "Untitled"}`
                  : "Plan history"}
            </span>
            {(pendingPlan || viewing) && (
              <span
                className={`rounded border px-1.5 py-0.5 text-[10px] ${
                  badge(viewing ? viewing.status : "presented").cls
                }`}
              >
                {badge(viewing ? viewing.status : "presented").label}
              </span>
            )}
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => {
                setViewing(null);
                setPlanHistoryOpen(!planHistoryOpen);
                if (!planHistoryOpen) void loadPlanHistory();
              }}
              className="rounded border border-[var(--border)] px-2 py-1 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)]"
            >
              {planHistoryOpen && !viewing ? "Back" : "History"}
            </button>
            <button
              type="button"
              onClick={() => void closePlanModal()}
              className="rounded border border-[var(--border)] px-2 py-1 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)]"
            >
              Close
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="flex min-h-0 flex-1">
          {/* Main: editor, diff or history */}
          <div className="flex min-w-0 flex-1 flex-col">
            {viewing ? (
              <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-4 py-3 font-mono text-[12px] leading-relaxed text-[var(--text)]">
                {viewing.plan}
              </pre>
            ) : planHistoryOpen ? (
              <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
                {planHistory.length === 0 ? (
                  <div className="px-2 py-6 text-center text-[11px] text-[var(--text-faint)]">
                    No plans yet. Plans the agent presents will appear here.
                  </div>
                ) : (
                  planHistory.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => void openHistory(p.id)}
                      className="mb-1 flex w-full items-center justify-between gap-2 rounded border border-transparent px-2 py-1.5 text-left hover:border-[var(--border)] hover:bg-[var(--bg)]"
                    >
                      <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--text)]">
                        {p.title ?? "Untitled"}
                      </span>
                      <span className={`rounded border px-1.5 py-0.5 text-[10px] ${badge(p.status).cls}`}>
                        {badge(p.status).label}
                      </span>
                      <span className="shrink-0 text-[10px] text-[var(--text-faint)]">
                        {p.updated_at ? formatTimestamp(p.updated_at) : ""}
                      </span>
                    </button>
                  ))
                )}
              </div>
            ) : pendingPlan ? (
              <>
                {/* Main View Mode Selector */}
                <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--bg)]/50 px-3 py-1.5 text-[11px]">
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      onClick={() => setViewMode("edit")}
                      className={`rounded px-2.5 py-1 font-medium transition ${
                        viewMode === "edit"
                          ? "bg-[var(--surface-3)] text-white shadow-xs"
                          : "text-[var(--text-faint)] hover:bg-[var(--surface)] hover:text-[var(--text)]"
                      }`}
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      onClick={() => setViewMode("preview")}
                      className={`rounded px-2.5 py-1 font-medium transition ${
                        viewMode === "preview"
                          ? "bg-[var(--surface-3)] text-white shadow-xs"
                          : "text-[var(--text-faint)] hover:bg-[var(--surface)] hover:text-[var(--text)]"
                      }`}
                    >
                      Preview
                    </button>
                    {previousPlanText && (
                      <button
                        type="button"
                        onClick={() => setViewMode("diff")}
                        className={`flex items-center gap-1.5 rounded px-2.5 py-1 font-medium transition ${
                          viewMode === "diff"
                            ? "bg-[var(--surface-3)] text-white shadow-xs"
                            : "text-[var(--text-faint)] hover:bg-[var(--surface)] hover:text-[var(--text)]"
                        }`}
                      >
                        Diff changes
                        {diff.hasChanges && (
                          <span className="flex items-center gap-1 font-mono text-[10px]">
                            {diff.addedCount > 0 && <span className="text-emerald-400">+{diff.addedCount}</span>}
                            {diff.removedCount > 0 && <span className="text-rose-400">-{diff.removedCount}</span>}
                          </span>
                        )}
                      </button>
                    )}
                  </div>
                  {viewMode === "diff" && diff.hasChanges && (
                    <div className="text-[10px] text-[var(--text-faint)]">
                      Diff against previous plan version
                    </div>
                  )}
                </div>

                {/* Content View */}
                {viewMode === "diff" && previousPlanText ? (
                  <div className="min-h-0 flex-1 overflow-auto bg-[#0a0e14] py-2 font-mono text-[12px] leading-relaxed">
                    {diff.lines.map((line, idx) => (
                      <div
                        key={idx}
                        className={`flex items-start px-2 py-0.5 ${
                          line.kind === "add"
                            ? "bg-emerald-950/40 text-emerald-200 border-l-2 border-emerald-500"
                            : line.kind === "del"
                              ? "bg-rose-950/40 text-rose-300 border-l-2 border-rose-500 opacity-85"
                              : "text-gray-300 border-l-2 border-transparent"
                        }`}
                      >
                        <span className="w-8 shrink-0 select-none text-right pr-2 text-[10px] text-gray-600">
                          {line.oldLineNumber ?? ""}
                        </span>
                        <span className="w-8 shrink-0 select-none text-right pr-2 text-[10px] text-gray-600">
                          {line.newLineNumber ?? ""}
                        </span>
                        <span
                          className={`w-5 shrink-0 select-none text-center font-bold ${
                            line.kind === "add"
                              ? "text-emerald-400"
                              : line.kind === "del"
                                ? "text-rose-400"
                                : "text-gray-600"
                          }`}
                        >
                          {line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}
                        </span>
                        <span className="flex-1 whitespace-pre-wrap break-all">{line.text || "\u00A0"}</span>
                      </div>
                    ))}
                  </div>
                ) : viewMode === "preview" ? (
                  <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-4 py-3 font-mono text-[12px] leading-relaxed text-[var(--text)]">
                    {planDraft}
                  </pre>
                ) : (
                  <textarea
                    value={planDraft}
                    onChange={(e) => setPlanDraft(e.target.value)}
                    spellCheck={false}
                    disabled={sending || planRevising}
                    className="min-h-0 flex-1 resize-none bg-transparent px-4 py-3 font-mono text-[12px] leading-relaxed text-[var(--text)] outline-none disabled:opacity-60"
                  />
                )}
              </>
            ) : (
              <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
                <div className="px-2 py-6 text-center text-[11px] text-[var(--text-faint)]">
                  No plans yet. Plans the agent presents will appear here.
                </div>
              </div>
            )}
          </div>

          {/* Side: revision chat + actions (only for the active pending plan) */}
          {pendingPlan && !viewing && !planHistoryOpen && (
            <div className="flex w-72 shrink-0 flex-col border-l border-[var(--border)] bg-[var(--surface-1)]">
              <div className="border-b border-[var(--border)] px-3 py-2 text-[10px] font-medium uppercase tracking-wide text-[var(--text-faint)] flex items-center justify-between">
                <span>Refine with the agent</span>
                {planRevising && (
                  <span className="flex items-center gap-1 text-amber-400 text-[10px] font-normal normal-case">
                    <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-400" />
                    Running
                  </span>
                )}
              </div>

              <div className="flex min-h-0 flex-1 flex-col gap-2.5 px-3 py-2.5 overflow-y-auto">
                {/* Live Progress Card when agent is revising */}
                {planRevising && (
                  <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-2.5 flex flex-col gap-2">
                    <div className="flex items-center gap-2 text-[11px] font-semibold text-amber-300">
                      <span className="relative flex h-2 w-2">
                        <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-amber-400 opacity-75"></span>
                        <span className="relative inline-flex rounded-full h-2 w-2 bg-amber-500"></span>
                      </span>
                      Agent is revising plan…
                    </div>

                    {activeActivityLabel && (
                      <div className="text-[10px] text-amber-200/90 font-mono break-all leading-tight">
                        ⚙️ {activeActivityLabel}
                      </div>
                    )}

                    {streamingSnippet && (
                      <div className="max-h-24 overflow-y-auto rounded bg-black/40 p-2 font-mono text-[10px] text-gray-300 leading-tight">
                        {streamingSnippet}
                      </div>
                    )}

                    <button
                      type="button"
                      onClick={() => void stopPlanRevision()}
                      className="flex items-center justify-center gap-1.5 rounded bg-rose-600/90 hover:bg-rose-600 py-1.5 px-2.5 text-[11px] font-semibold text-white transition shadow-xs"
                    >
                      <StopIcon size={12} />
                      Stop Agent
                    </button>
                  </div>
                )}

                <textarea
                  value={feedback}
                  onChange={(e) => setFeedback(e.target.value)}
                  placeholder="e.g. add a rollback step, split into phases, use less downtime…"
                  rows={5}
                  disabled={sending || planRevising}
                  className="resize-none rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-2 font-mono text-[11px] text-[var(--text)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent)] disabled:opacity-50"
                />

                <button
                  type="button"
                  disabled={sending || planRevising || !feedback.trim()}
                  onClick={() => void sendRevision("feedback")}
                  className="rounded bg-[var(--accent-muted)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--accent)] hover:bg-[var(--accent)] hover:text-black transition disabled:opacity-40"
                >
                  {sending || planRevising ? "Sending…" : "Send changes"}
                </button>

                <button
                  type="button"
                  disabled={sending || planRevising}
                  onClick={() => void sendRevision("apply-draft")}
                  className="rounded border border-[var(--border)] px-2.5 py-1.5 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)] hover:text-white transition disabled:opacity-40"
                >
                  Submit my edited plan
                </button>

                <div className="mt-auto border-t border-[var(--border)] pt-2 text-[10px] leading-relaxed text-[var(--text-faint)]">
                  The agent revises and re-presents the plan here. Nothing runs until you apply.
                </div>
              </div>

              {/* Action Buttons */}
              <div className="flex flex-col gap-2 border-t border-[var(--border)] p-3 bg-[var(--surface-2)]">
                <button
                  type="button"
                  disabled={sending || planRevising}
                  onClick={() => void applyPlan(pendingPlan.id)}
                  className="rounded bg-emerald-600/90 px-3 py-2 text-[12px] font-semibold text-white hover:bg-emerald-600 disabled:opacity-40 flex items-center justify-center gap-1.5 transition shadow-sm"
                >
                  {planRevising ? (
                    <>
                      <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-white" />
                      Agent is revising…
                    </>
                  ) : sending ? (
                    "Working…"
                  ) : (
                    "Apply & run"
                  )}
                </button>

                <div className="flex gap-2">
                  <button
                    type="button"
                    disabled={sending || planRevising}
                    onClick={() => void archivePlanAction(pendingPlan.id)}
                    className="flex-1 rounded border border-[var(--border)] px-2 py-1.5 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)] hover:text-white transition disabled:opacity-40"
                  >
                    Archive
                  </button>
                  <button
                    type="button"
                    disabled={sending || planRevising}
                    onClick={() => void cancelPlanAction(pendingPlan.id)}
                    className="flex-1 rounded border border-[var(--border)] px-2 py-1.5 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)] hover:text-white transition disabled:opacity-40"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
