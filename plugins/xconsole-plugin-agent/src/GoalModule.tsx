import { useState } from "react";
import type { GoalTaskItem } from "./types";

export function GoalModule({
  activeGoal,
  tasks,
  onSaveGoal,
  onToggleTask,
  onAddTask,
}: {
  activeGoal: string;
  tasks: GoalTaskItem[];
  onSaveGoal: (goal: string) => void;
  onToggleTask: (id: string) => void;
  onAddTask: (text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [input, setInput] = useState(activeGoal);
  const [newTaskText, setNewTaskText] = useState("");

  const handleSave = () => {
    onSaveGoal(input);
    setEditing(false);
  };

  const handleAddTask = (e: React.FormEvent) => {
    e.preventDefault();
    if (newTaskText.trim()) {
      onAddTask(newTaskText.trim());
      setNewTaskText("");
    }
  };

  return (
    <div className="border-b border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-xs font-mono">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 flex-1 min-w-0">
          <span className="text-amber-400 font-semibold">❯</span>
          <span className="text-[10px] text-[var(--text-dim)] uppercase tracking-wider font-semibold">
            GOAL:
          </span>

          {editing ? (
            <div className="flex items-center gap-1 flex-1">
              <input
                className="w-full rounded border border-amber-500/50 bg-[var(--surface-2)] px-2 py-0.5 text-xs text-[var(--text)] focus:outline-none"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleSave()}
                autoFocus
              />
              <button
                className="rounded bg-amber-500/20 text-amber-300 px-2 py-0.5 text-[11px]"
                onClick={handleSave}
              >
                Save
              </button>
            </div>
          ) : (
            <span
              className="cursor-pointer truncate text-[var(--text)] hover:text-amber-300"
              onClick={() => setEditing(true)}
              title="Click to edit goal"
            >
              {activeGoal || "No goal defined. Click to set objective..."}
            </span>
          )}
        </div>
      </div>

      {tasks.length > 0 && (
        <div className="mt-2 space-y-1">
          {tasks.map((t) => (
            <label key={t.id} className="flex items-center gap-2 cursor-pointer text-[11px]">
              <input
                type="checkbox"
                checked={t.done}
                onChange={() => onToggleTask(t.id)}
                className="rounded"
              />
              <span className={t.done ? "line-through text-gray-500" : "text-gray-300"}>{t.text}</span>
            </label>
          ))}
        </div>
      )}

      <form onSubmit={handleAddTask} className="mt-2 flex gap-1">
        <input
          type="text"
          className="flex-1 rounded border border-[var(--border)] bg-[var(--surface-2)] px-2 py-0.5 text-[11px] text-gray-200"
          placeholder="+ Add task..."
          value={newTaskText}
          onChange={(e) => setNewTaskText(e.target.value)}
        />
      </form>
    </div>
  );
}

