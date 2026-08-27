import type { GoalSpec } from "../../../src/lib/tauri";

/** Lock CTA shown in chat and on the goal board during intake. */
export function GoalLockCard({
  spec,
  onLock,
  onCancel,
}: {
  spec: GoalSpec | null;
  onLock: () => void;
  onCancel?: () => void;
}) {
  return (
    <div className="rounded-md border border-emerald-500/40 bg-emerald-500/10 p-2">
      <div className="mb-1 text-[11px] font-medium text-emerald-200">
        {spec ? "Lock this goal and start?" : "Goal intake is open"}
      </div>
      {spec ? (
        <div className="mb-2 space-y-0.5 text-[11px] text-gray-300">
          <div>
            <span className="text-[var(--text-faint)]">objective: </span>
            {spec.objective}
          </div>
          {(spec.success_criteria ?? []).length > 0 && (
            <div>
              <span className="text-[var(--text-faint)]">done when: </span>
              {spec.success_criteria.join("; ")}
            </div>
          )}
          {spec.check_method ? (
            <div>
              <span className="text-[var(--text-faint)]">check: </span>
              {spec.check_method}
            </div>
          ) : null}
        </div>
      ) : (
        <div className="mb-2 text-[11px] text-[var(--text-faint)]">
          The agent is drafting the spec. This button stays here so you can lock it without
          hunting for the board.
        </div>
      )}
      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={onLock}
          disabled={!spec}
          className="rounded-md bg-emerald-600 px-2.5 py-1 text-[11px] text-white hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-50"
        >
          Lock goal & start
        </button>
        {onCancel && (
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border border-[var(--border)] px-2.5 py-1 text-[11px] text-gray-300 hover:bg-[var(--border)]"
          >
            Cancel
          </button>
        )}
      </div>
    </div>
  );
}

