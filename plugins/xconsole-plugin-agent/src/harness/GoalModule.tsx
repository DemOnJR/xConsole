import { useState, useRef, useEffect } from "react";
import { useHarnessStore } from "../../../../src/stores/harnessStore";
import { useAgentStore } from "../../../../src/stores/agentStore";

export function GoalModule() {
  const showGoal = useHarnessStore((s) => s.showGoal);
  const goalCollapsed = useHarnessStore((s) => s.goalCollapsed);
  const toggleGoalCollapsed = useHarnessStore((s) => s.toggleGoalCollapsed);

  const activeGoal = useHarnessStore((s) => s.activeGoal);
  const goalStatus = useHarnessStore((s) => s.goalStatus);
  const goalTasks = useHarnessStore((s) => s.goalTasks);
  const setActiveGoal = useHarnessStore((s) => s.setActiveGoal);
  const setGoalStatus = useHarnessStore((s) => s.setGoalStatus);
  const toggleGoalTask = useHarnessStore((s) => s.toggleGoalTask);
  const addGoalTask = useHarnessStore((s) => s.addGoalTask);
  const removeGoalTask = useHarnessStore((s) => s.removeGoalTask);
  const clearGoal = useHarnessStore((s) => s.clearGoal);

  const sendAgentMessage = useAgentStore((s) => s.send);

  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState(activeGoal);
  const [newTaskText, setNewTaskText] = useState("");
  const editInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setEditText(activeGoal);
  }, [activeGoal]);

  useEffect(() => {
    if (isEditing) {
      editInputRef.current?.focus();
      editInputRef.current?.select();
    }
  }, [isEditing]);

  if (!showGoal) return null;

  const completedCount = goalTasks.filter((t) => t.done).length;
  const totalCount = goalTasks.length;
  const percent = totalCount > 0 ? Math.round((completedCount / totalCount) * 100) : 0;

  const handleSaveGoal = () => {
    const trimmed = editText.trim();
    setActiveGoal(trimmed);
    setIsEditing(false);
    if (trimmed && trimmed !== activeGoal) {
      // Optional trigger agent goal instruction
      setGoalStatus("running");
    }
  };

  const handleAddTask = (e: React.FormEvent) => {
    e.preventDefault();
    if (newTaskText.trim()) {
      addGoalTask(newTaskText.trim());
      setNewTaskText("");
    }
  };

  return (
    <div className="border-b border-[var(--border)] bg-[var(--surface)]/90 text-xs font-mono transition-all">
      {/* Header bar / Collapsed view */}
      <div className="flex items-center justify-between px-3 py-2 gap-2">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <span className="text-amber-400 select-none font-semibold">❯</span>
          <span className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider font-semibold">
            GOAL:
          </span>

          {isEditing ? (
            <div className="flex items-center gap-1.5 flex-1 min-w-0">
              <input
                ref={editInputRef}
                type="text"
                className="w-full rounded border border-amber-500/50 bg-[var(--surface-2)] px-2 py-0.5 text-xs text-[var(--text)] focus:outline-none focus:ring-1 focus:ring-amber-400 font-mono"
                placeholder="Define active objective... (e.g. 'Refactor auth module & add unit tests')"
                value={editText}
                onChange={(e) => setEditText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleSaveGoal();
                  if (e.key === "Escape") setIsEditing(false);
                }}
              />
              <button
                type="button"
                className="rounded bg-amber-500/20 text-amber-300 border border-amber-500/40 px-2 py-0.5 text-[11px] hover:bg-amber-500/30"
                onClick={handleSaveGoal}
              >
                Save
              </button>
              <button
                type="button"
                className="rounded text-[var(--text-dim)] hover:text-[var(--text)] px-1.5 py-0.5 text-[11px]"
                onClick={() => setIsEditing(false)}
              >
                Cancel
              </button>
            </div>
          ) : (
            <div
              className="flex items-center gap-2 truncate cursor-pointer group flex-1 min-w-0"
              onClick={() => setIsEditing(true)}
              title="Click to edit objective"
            >
              <span className={`truncate font-medium ${activeGoal ? "text-[var(--text)]" : "text-[var(--text-dim)] italic"}`}>
                {activeGoal || "No active goal defined. Click to set objective..."}
              </span>
              <span className="opacity-0 group-hover:opacity-100 text-[10px] text-amber-400/80 transition">
                ✎ edit
              </span>
            </div>
          )}
        </div>

        {/* Status Badge & Actions */}
        <div className="flex items-center gap-2 shrink-0">
          {activeGoal && (
            <>
              {totalCount > 0 && (
                <span className="text-[10px] text-[var(--text-dim)] bg-[var(--surface-2)] border border-[var(--border)] px-1.5 py-0.2 rounded font-mono">
                  {completedCount}/{totalCount} ({percent}%)
                </span>
              )}

              <span
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] uppercase font-mono border ${
                  goalStatus === "running"
                    ? "bg-amber-500/10 text-amber-300 border-amber-500/30"
                    : goalStatus === "completed"
                      ? "bg-emerald-500/10 text-emerald-300 border-emerald-500/30"
                      : goalStatus === "paused"
                        ? "bg-blue-500/10 text-blue-300 border-blue-500/30"
                        : "bg-gray-500/10 text-gray-400 border-gray-500/30"
                }`}
              >
                {goalStatus === "running" && <span className="animate-spin text-[10px]">⠋</span>}
                {goalStatus === "completed" && <span>✓</span>}
                {goalStatus === "paused" && <span>⏸</span>}
                <span>{goalStatus}</span>
              </span>
            </>
          )}

          <button
            type="button"
            className="text-[var(--text-dim)] hover:text-[var(--text)] p-1 rounded hover:bg-[var(--surface-2)] transition"
            onClick={toggleGoalCollapsed}
            title={goalCollapsed ? "Expand Goal details" : "Collapse Goal details"}
          >
            {goalCollapsed ? "▼" : "▲"}
          </button>
        </div>
      </div>

      {/* Expanded Goal Checklist & Controls */}
      {!goalCollapsed && activeGoal && (
        <div className="border-t border-[var(--border)]/40 bg-[var(--surface-2)]/30 px-3 py-2.5 space-y-2">
          {/* Subtasks List */}
          {goalTasks.length > 0 && (
            <div className="space-y-1">
              <div className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider font-semibold mb-1">
                Milestones &amp; Checklist:
              </div>
              <div className="space-y-1 max-h-36 overflow-y-auto pr-1">
                {goalTasks.map((task) => (
                  <div
                    key={task.id}
                    className="flex items-center justify-between gap-2 group/task rounded px-1.5 py-0.5 hover:bg-[var(--surface)] text-[11px]"
                  >
                    <label className="flex items-center gap-2 cursor-pointer min-w-0 flex-1">
                      <input
                        type="checkbox"
                        checked={task.done}
                        onChange={() => toggleGoalTask(task.id)}
                        className="rounded border-[var(--border)] text-amber-500 focus:ring-0 focus:ring-offset-0 bg-transparent h-3.5 w-3.5"
                      />
                      <span className={`truncate ${task.done ? "line-through text-[var(--text-dim)]" : "text-[var(--text)]"}`}>
                        {task.text}
                      </span>
                    </label>
                    <button
                      type="button"
                      className="opacity-0 group-hover/task:opacity-100 text-[var(--text-dim)] hover:text-red-400 text-xs px-1"
                      onClick={() => removeGoalTask(task.id)}
                      title="Remove task"
                    >
                      ✕
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Add Subtask Form */}
          <form onSubmit={handleAddTask} className="flex items-center gap-1.5 pt-1">
            <input
              type="text"
              className="flex-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-1 text-[11px] text-[var(--text)] placeholder-[var(--text-dim)] focus:outline-none focus:border-amber-400/50"
              placeholder="+ Add sub-task milestone... (press Enter)"
              value={newTaskText}
              onChange={(e) => setNewTaskText(e.target.value)}
            />
            {newTaskText.trim() && (
              <button
                type="submit"
                className="rounded bg-amber-500/20 text-amber-300 border border-amber-500/40 px-2 py-1 text-[11px] hover:bg-amber-500/30"
              >
                Add
              </button>
            )}
          </form>

          {/* Bottom Quick Controls */}
          <div className="flex items-center justify-between pt-1 border-t border-[var(--border)]/30 text-[10px] text-[var(--text-dim)]">
            <div className="flex items-center gap-2">
              <button
                type="button"
                className="hover:text-[var(--text)] underline"
                onClick={() => {
                  if (goalStatus === "running") setGoalStatus("paused");
                  else if (goalStatus === "paused") setGoalStatus("running");
                  else setGoalStatus("completed");
                }}
              >
                {goalStatus === "running" ? "⏸ Pause goal" : goalStatus === "paused" ? "▶ Resume goal" : "✓ Mark complete"}
              </button>
              <span>&bull;</span>
              <button
                type="button"
                className="hover:text-[var(--text)] underline"
                onClick={() => {
                  void sendAgentMessage(`Focus on completing our current goal: ${activeGoal}`);
                }}
              >
                ⚡ Prompt agent on goal
              </button>
            </div>
            <button
              type="button"
              className="text-red-400/80 hover:text-red-300"
              onClick={clearGoal}
            >
              Clear goal
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
