import { useEffect, useMemo, useRef, useState } from "react";
import { NodeResizer, useStore, type NodeProps } from "@xyflow/react";
import { api, onGoalEvent, type GoalMemory, type GoalSession, type GoalSpec, type GoalTask } from "../lib/tauri";
import { useCanvasStore, type GoalNode as GoalNodeType } from "../stores/canvasStore";
import { useGoalStore } from "../stores/goalStore";
import { useAgentStore } from "../stores/agentStore";

const COLUMNS = ["backlog", "in_progress", "waiting", "testing", "blocked", "done"];

const KIND_COLOR: Record<string, string> = {
  edit: "text-amber-300",
  test: "text-emerald-300",
  bug: "text-red-300",
  research: "text-blue-300",
  check: "text-cyan-300",
};

function parseSpec(s: GoalSession): GoalSpec | null {
  try {
    return JSON.parse(s.spec_json) as GoalSpec;
  } catch {
    return null;
  }
}

function parseTasks(s: GoalSession): GoalTask[] {
  try {
    return JSON.parse(s.kanban_json) as GoalTask[];
  } catch {
    return [];
  }
}

function parseMemory(s: GoalSession): GoalMemory {
  try {
    return JSON.parse(s.memory_json) as GoalMemory;
  } catch {
    return { learned: [] };
  }
}

/** Kanban board node for a /goal session. Live-updates via goal:// events. */
export function GoalNode({ id, data, selected }: NodeProps<GoalNodeType>) {
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);

  const goalId = data.goalId;
  const [session, setSession] = useState<GoalSession | null>(null);
  const [spec, setSpec] = useState<GoalSpec | null>(null);
  const [tasks, setTasks] = useState<GoalTask[]>([]);
  const [memory, setMemory] = useState<GoalMemory>({ learned: [] });
  const [error, setError] = useState<string | null>(null);
  const [countdown, setCountdown] = useState<string>("");
  const intervalRef = useRef<number | null>(null);

  // Load the session on mount + refresh on goal events.
  useEffect(() => {
    let alive = true;
    let un: (() => void) | undefined;
    (async () => {
      try {
        const s = await api.getGoal(goalId);
        if (!alive) return;
        setSession(s);
        setSpec(parseSpec(s));
        setTasks(parseTasks(s));
        setMemory(parseMemory(s));
      } catch (e) {
        if (alive) setError(String(e));
      }
      un = await onGoalEvent(goalId, () => {
        void api
          .getGoal(goalId)
          .then((s) => {
            if (!alive) return;
            setSession(s);
            setSpec(parseSpec(s));
            setTasks(parseTasks(s));
            setMemory(parseMemory(s));
          })
          .catch(() => {});
      });
    })();
    return () => {
      alive = false;
      un?.();
      if (intervalRef.current != null) window.clearInterval(intervalRef.current);
    };
  }, [goalId]);

  // Countdown to next_check_at when waiting.
  useEffect(() => {
    if (!session || session.status !== "waiting" || !session.next_check_at) return;
    const tick = () => {
      const target = new Date(session.next_check_at!).getTime();
      const diff = target - Date.now();
      if (diff <= 0) {
        setCountdown("due now");
        return;
      }
      const h = Math.floor(diff / 3600000);
      const m = Math.floor((diff % 3600000) / 60000);
      setCountdown(h > 0 ? `${h}h ${m}m` : `${m}m`);
    };
    tick();
    intervalRef.current = window.setInterval(tick, 30000);
    return () => {
      if (intervalRef.current != null) window.clearInterval(intervalRef.current);
    };
  }, [session?.status, session?.next_check_at]);

  const byColumn = useMemo(() => {
    const map = new Map<string, GoalTask[]>();
    for (const c of COLUMNS) map.set(c, []);
    for (const t of tasks) {
      const list = map.get(t.column) ?? [];
      list.push(t);
      map.set(t.column, list);
    }
    return map;
  }, [tasks]);

  const statusTone: Record<string, string> = {
    intake: "text-amber-300",
    active: "text-emerald-300",
    waiting: "text-blue-300",
    blocked: "text-red-300",
    done: "text-green-300",
    stopped: "text-gray-400",
  };

  const onConfirm = async () => {
    try {
      await useGoalStore.getState().confirm(goalId);
      useAgentStore.getState().setActiveIntakeGoal(null);
    } catch (e) {
      setError(String(e));
    }
  };
  const onStop = async () => {
    try {
      await useGoalStore.getState().stop(goalId);
      useAgentStore.getState().setActiveIntakeGoal(null);
    } catch (e) {
      setError(String(e));
    }
  };
  const onClose = () => removeNode(id);

  return (
    <div
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-blue-500" : "border-[var(--border)]"}`}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
      onMouseDown={() => focus(id)}
    >
      <NodeResizer minWidth={420} minHeight={240} isVisible lineClassName="!border-blue-500" handleClassName="!bg-blue-500" />

      {/* Header */}
      <div className="flex cursor-move select-none items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 font-mono text-[11px]">
        <span className="text-violet-400">⬡</span>
        <span className="truncate text-gray-200">{session?.title ?? "Goal"}</span>
        {session && (
          <span className={`rounded px-1 text-[9px] ${statusTone[session.status] ?? "text-gray-400"}`}>
            {session.status}
          </span>
        )}
        {session && session.cycles > 0 && (
          <span className="text-[9px] text-[var(--text-faint)]">{session.cycles} cycles</span>
        )}
        {session?.status === "waiting" && countdown && (
          <span className="text-[9px] text-blue-300" data-tooltip="Waiting — resume countdown">
            ⟳ {countdown}
          </span>
        )}
        <span className="ml-auto flex items-center gap-1">
          {session?.status === "intake" && (
            <button
              type="button"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => void onConfirm()}
              className="rounded border border-emerald-500/40 bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-300 hover:bg-emerald-500/20"
            >
              Lock goal &amp; start
            </button>
          )}
          {session && session.status !== "done" && session.status !== "stopped" && (
            <button
              type="button"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => void onStop()}
              className="rounded border border-red-500/40 px-1.5 py-0.5 text-[10px] text-red-300 hover:bg-red-500/20"
            >
              stop
            </button>
          )}
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={onClose}
            className="rounded px-1 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
          >
            ✕
          </button>
        </span>
      </div>

      {/* Objective (when locked) */}
      {spec && (
        <div className="border-b border-[var(--border)]/60 px-3 py-1.5 text-[10px] text-[var(--text-dim)]">
          <span className="text-[var(--text-faint)]">objective: </span>
          {spec.objective}
          {spec.success_criteria.length > 0 && (
            <span className="text-[var(--text-faint)]">
              {" "}· done when: {spec.success_criteria.join("; ")}
            </span>
          )}
        </div>
      )}

      {error && <div className="border-b border-red-900/30 px-3 py-1 text-[10px] text-red-400">{error}</div>}

      {/* Kanban board */}
      <div className="nodrag nowheel flex min-h-0 flex-1 gap-1.5 overflow-x-auto px-2 py-2">
        {COLUMNS.map((col) => (
          <div key={col} className="flex min-w-[110px] flex-1 flex-col gap-1 rounded border border-[var(--border)]/60 bg-[var(--surface)]/40 p-1">
            <div className="text-center text-[9px] uppercase tracking-wide text-[var(--text-faint)]">
              {col}
            </div>
            {(byColumn.get(col) ?? []).map((t) => (
              <div key={t.id} className="rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-1">
                <div className="flex items-center gap-1">
                  {t.kind && (
                    <span className={`text-[9px] ${KIND_COLOR[t.kind] ?? "text-gray-400"}`}>
                      {t.kind}
                    </span>
                  )}
                  <span className="min-w-0 flex-1 truncate text-[10px] text-gray-200" data-tooltip={t.detail ?? undefined}>
                    {t.title}
                  </span>
                </div>
                {t.files && t.files.length > 0 && (
                  <div className="mt-0.5 truncate text-[9px] text-[var(--text-faint)]">
                    {t.files.join(", ")}
                  </div>
                )}
                {t.result && <div className="mt-0.5 truncate text-[9px] text-emerald-300/80">{t.result}</div>}
                {t.error && <div className="mt-0.5 truncate text-[9px] text-red-300/80">{t.error}</div>}
              </div>
            ))}
          </div>
        ))}
      </div>

      {/* Constraint memory strip */}
      {memory.learned.length > 0 && (
        <div className="border-t border-[var(--border)]/60 px-3 py-1 text-[9px] text-[var(--text-faint)]">
          {memory.learned.map((l) => (
            <span key={l.key} className="mr-2" data-tooltip={l.evidence}>
              {l.key}: {l.value} <span className="text-[var(--text-faint)]/60">({l.confidence})</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
