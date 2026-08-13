import { useEffect, useState } from "react";
import { useAgentStore } from "../../stores/agentStore";
import { api, type AgentPlanFull } from "../../lib/tauri";

const STATUS_BADGE: Record<string, { label: string; cls: string }> = {
  presented: { label: "Awaiting review", cls: "text-amber-300 border-amber-500/40 bg-amber-500/10" },
  applied: { label: "Applied", cls: "text-emerald-300 border-emerald-500/40 bg-emerald-500/10" },
  archived: { label: "Archived", cls: "text-gray-400 border-gray-500/40 bg-gray-500/10" },
  cancelled: { label: "Cancelled", cls: "text-red-300 border-red-500/40 bg-red-500/10" },
};

/**
 * Full-window plan review modal, rendered above the terminal canvas.
 * The plan is editable; revisions are sent back to the agent as feedback and
 * the agent re-presents through `present_plan` (new ai://plan event swaps it).
 */
export function PlanModal() {
  const pendingPlan = useAgentStore((s) => s.pendingPlan);
  const planDraft = useAgentStore((s) => s.planDraft);
  const planHistory = useAgentStore((s) => s.planHistory);
  const planHistoryOpen = useAgentStore((s) => s.planHistoryOpen);
  const setPlanDraft = useAgentStore((s) => s.setPlanDraft);
  const applyPlan = useAgentStore((s) => s.applyPlan);
  const archivePlanAction = useAgentStore((s) => s.archivePlanAction);
  const cancelPlanAction = useAgentStore((s) => s.cancelPlanAction);
  const revisePlan = useAgentStore((s) => s.revisePlan);
  const loadPlanHistory = useAgentStore((s) => s.loadPlanHistory);
  const setPlanHistoryOpen = useAgentStore((s) => s.setPlanHistoryOpen);
  const closePlanModal = useAgentStore((s) => s.closePlanModal);

  const [feedback, setFeedback] = useState("");
  const [viewing, setViewing] = useState<AgentPlanFull | null>(null);
  const [sending, setSending] = useState(false);

  const open = !!pendingPlan || planHistoryOpen;

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
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
    } finally {
      setSending(false);
    }
  };

  const openHistory = async (id: string) => {
    const full = await api.getPlan(id).catch(() => null);
    setViewing(full);
  };

  const badge = (status: string) => STATUS_BADGE[status] ?? STATUS_BADGE.presented;

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
          {/* Main: editor or history */}
          <div className="flex min-w-0 flex-1 flex-col">
            {viewing ? (
              <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-4 py-3 font-mono text-[12px] leading-relaxed text-[var(--text)]">
                {viewing.plan}
              </pre>
            ) : pendingPlan ? (
              <textarea
                value={planDraft}
                onChange={(e) => setPlanDraft(e.target.value)}
                spellCheck={false}
                className="min-h-0 flex-1 resize-none bg-transparent px-4 py-3 font-mono text-[12px] leading-relaxed text-[var(--text)] outline-none"
              />
            ) : (
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
                        {p.updated_at ? new Date(p.updated_at).toLocaleString() : ""}
                      </span>
                    </button>
                  ))
                )}
              </div>
            )}
          </div>

          {/* Side: revision chat + actions (only for the active pending plan) */}
          {pendingPlan && !viewing && (
            <div className="flex w-64 shrink-0 flex-col border-l border-[var(--border)]">
              <div className="border-b border-[var(--border)] px-3 py-2 text-[10px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                Refine with the agent
              </div>
              <div className="flex min-h-0 flex-1 flex-col gap-2 px-3 py-2">
                <textarea
                  value={feedback}
                  onChange={(e) => setFeedback(e.target.value)}
                  placeholder="e.g. add a rollback step, split into phases, use less downtime…"
                  rows={6}
                  className="resize-none rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5 font-mono text-[11px] text-[var(--text)] outline-none placeholder:text-[var(--text-faint)]"
                />
                <button
                  type="button"
                  disabled={sending || !feedback.trim()}
                  onClick={() => void sendRevision("feedback")}
                  className="rounded bg-[var(--accent-muted)] px-2 py-1.5 text-[11px] font-medium text-[var(--accent)] disabled:opacity-40"
                >
                  {sending ? "Sending…" : "Send changes"}
                </button>
                <button
                  type="button"
                  disabled={sending}
                  onClick={() => void sendRevision("apply-draft")}
                  className="rounded border border-[var(--border)] px-2 py-1.5 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)] disabled:opacity-40"
                >
                  Submit my edited plan
                </button>
                <div className="mt-2 border-t border-[var(--border)] pt-2 text-[10px] leading-relaxed text-[var(--text-faint)]">
                  The agent revises and re-presents the plan here. Nothing runs until you apply.
                </div>
              </div>
              <div className="flex flex-col gap-1.5 border-t border-[var(--border)] px-3 py-2">
                <button
                  type="button"
                  onClick={() => void applyPlan(pendingPlan.id)}
                  className="rounded bg-emerald-600/90 px-2 py-1.5 text-[12px] font-semibold text-white hover:bg-emerald-600"
                >
                  Apply & run
                </button>
                <div className="flex gap-1.5">
                  <button
                    type="button"
                    onClick={() => void archivePlanAction(pendingPlan.id)}
                    className="flex-1 rounded border border-[var(--border)] px-2 py-1.5 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)]"
                  >
                    Archive
                  </button>
                  <button
                    type="button"
                    onClick={() => void cancelPlanAction(pendingPlan.id)}
                    className="flex-1 rounded border border-[var(--border)] px-2 py-1.5 text-[11px] text-[var(--text-faint)] hover:bg-[var(--border)]"
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
