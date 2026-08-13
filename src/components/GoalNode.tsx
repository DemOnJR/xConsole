import { useEffect, useMemo, useRef, useState } from "react";
import { NodeResizer, useStore, type NodeProps } from "@xyflow/react";
import { api, onGoalEvent, type GoalMemory, type GoalSession, type GoalSpec, type GoalTask } from "../lib/tauri";
import { goalRootTasks, goalTaskChildren, parseGoalSessionViews } from "../lib/goalParse";
import { useCanvasStore, type GoalNode as GoalNodeType } from "../stores/canvasStore";
import { useGoalStore } from "../stores/goalStore";
import { useAgentStore } from "../stores/agentStore";
import { NodeErrorBoundary } from "./NodeErrorBoundary";
import { GoalLockCard } from "./agent/GoalLockCard";
import { GoalTaskModal } from "./GoalTaskModal";

const COLUMNS = ["backlog", "in_progress", "waiting", "testing", "blocked", "done"];
const COL_W = 168;

/** React Flow / WebView2 mark wheel listeners as passive, so overflow-auto never
 *  receives a usable scroll. Drive the nearest [data-goal-scroll] ourselves. */
function useGoalBoardScroll(deps: unknown) {
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const board = ref.current;
    if (!board) return;
    const onWheel = (e: WheelEvent) => {
      e.stopPropagation();
      const col = (e.target as HTMLElement | null)?.closest<HTMLElement>("[data-goal-col-scroll]");
      const dx = e.shiftKey ? e.deltaY : e.deltaX;
      const dy = e.shiftKey ? 0 : e.deltaY;
      let moved = false;
      if (col && dy) {
        const before = col.scrollTop;
        col.scrollTop += dy;
        moved = col.scrollTop !== before;
      }
      if ((!moved || !col) && dy) {
        const before = board.scrollTop;
        board.scrollTop += dy;
        if (board.scrollTop !== before) moved = true;
      }
      if (dx) {
        const before = board.scrollLeft;
        board.scrollLeft += dx;
        if (board.scrollLeft !== before) moved = true;
      }
      e.preventDefault();
    };
    board.addEventListener("wheel", onWheel, { capture: true, passive: false });
    return () => board.removeEventListener("wheel", onWheel, true);
  }, [deps]);
  return ref;
}

const KIND_COLOR: Record<string, string> = {
  edit: "text-amber-300",
  test: "text-emerald-300",
  bug: "text-red-300",
  research: "text-blue-300",
  check: "text-cyan-300",
};

function applySession(
  s: GoalSession,
  setSpec: (v: GoalSpec | null) => void,
  setTasks: (v: GoalTask[]) => void,
  setMemory: (v: GoalMemory) => void,
) {
  const views = parseGoalSessionViews(s);
  setSpec(views.spec);
  setTasks(views.tasks);
  setMemory(views.memory);
}

/** Kanban board node for a /goal session. Live-updates via goal:// events. */
export function GoalNode(props: NodeProps<GoalNodeType>) {
  return (
    <NodeErrorBoundary label="Goal">
      <GoalBoard {...props} />
    </NodeErrorBoundary>
  );
}

function GoalBoard({ id, data, selected }: NodeProps<GoalNodeType>) {
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]) || 1;

  const goalId = typeof data.goalId === "string" ? data.goalId : "";
  const [session, setSession] = useState<GoalSession | null>(null);
  const [spec, setSpec] = useState<GoalSpec | null>(null);
  const [tasks, setTasks] = useState<GoalTask[]>([]);
  const [memory, setMemory] = useState<GoalMemory>({ learned: [] });
  const [error, setError] = useState<string | null>(null);
  const [countdown, setCountdown] = useState<string>("");
  const [openTaskId, setOpenTaskId] = useState<string | null>(null);
  const intervalRef = useRef<number | null>(null);

  // Load the session on mount + refresh on goal events.
  useEffect(() => {
    if (!goalId) {
      setError("This goal board has no session id. Start a new /goal.");
      return;
    }
    let alive = true;
    let un: (() => void) | undefined;
    (async () => {
      try {
        const s = await api.getGoal(goalId);
        if (!alive) return;
        setSession(s);
        applySession(s, setSpec, setTasks, setMemory);
      } catch (e) {
        if (alive) setError(String(e));
      }
      un = await onGoalEvent(goalId, () => {
        void api
          .getGoal(goalId)
          .then((s) => {
            if (!alive) return;
            setSession(s);
            applySession(s, setSpec, setTasks, setMemory);
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
    for (const t of goalRootTasks(tasks)) {
      const list = map.get(t.column) ?? [];
      list.push(t);
      map.set(t.column, list);
    }
    return map;
  }, [tasks]);

  const statusTone: Record<string, string> = {
    intake: "text-amber-300",
    active: "text-emerald-300",
    paused: "text-amber-300",
    waiting: "text-blue-300",
    blocked: "text-red-300",
    done: "text-green-300",
    stopped: "text-gray-400",
  };

  const onConfirm = async () => {
    try {
      const targets = useAgentStore.getState().targets;
      await useGoalStore.getState().confirm(goalId, targets);
      useAgentStore.getState().setActiveIntakeGoal(null);
    } catch (e) {
      setError(String(e));
    }
  };
  const onPause = async () => {
    try {
      await useGoalStore.getState().pause(goalId);
    } catch (e) {
      setError(String(e));
    }
  };
  const onContinue = async () => {
    try {
      await useGoalStore.getState().resume(goalId);
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

  const boardRef = useGoalBoardScroll(tasks.length);

  return (
    <div
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-blue-500" : "border-[var(--border)]"}`}
      style={
        freeform || zoom === 1
          ? undefined
          : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }
      }
      onMouseDown={() => focus(id)}
    >
      <NodeResizer minWidth={420} minHeight={240} isVisible lineClassName="!border-blue-500" handleClassName="!bg-blue-500" />

      {/* Header */}
      <div className="flex flex-wrap cursor-move select-none items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 font-mono text-[11px]">
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
          {session?.status === "active" && (
            <button
              type="button"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => void onPause()}
              className="rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-200 hover:bg-amber-500/20"
            >
              pause
            </button>
          )}
          {session &&
            (session.status === "paused" ||
              session.status === "waiting" ||
              session.status === "blocked") && (
            <button
              type="button"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => void onContinue()}
              className="rounded border border-emerald-500/40 bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-300 hover:bg-emerald-500/20"
            >
              continue
            </button>
          )}
          {session && session.status !== "done" && session.status !== "stopped" && (
            <button
              type="button"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => void onStop()}
              className="rounded border border-red-500/40 px-1.5 py-0.5 text-[10px] text-red-300 hover:bg-red-500/20"
            >
              terminate
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
          {(spec.success_criteria ?? []).length > 0 && (
            <span className="text-[var(--text-faint)]">
              {" "}· done when: {spec.success_criteria.join("; ")}
            </span>
          )}
        </div>
      )}

      {error && <div className="border-b border-red-900/30 px-3 py-1 text-[10px] text-red-400">{error}</div>}

      {session?.status === "intake" && (
        <div className="nodrag border-b border-[var(--border)] px-3 py-2">
          <GoalLockCard spec={spec} onLock={() => void onConfirm()} onCancel={() => void onStop()} />
        </div>
      )}

      {/* Kanban: h-0 forces a real height so overflow scrollbars exist.
          Wheel is handled in useGoalBoardScroll — RF/WebView2 eat native wheel. */}
      <div
        ref={boardRef}
        data-goal-scroll
        className="nodrag nopan nowheel h-0 min-h-0 flex-1 overflow-x-auto overflow-y-auto px-2 py-2"
        onPointerDown={(e) => e.stopPropagation()}
        style={{ touchAction: "pan-x pan-y", overscrollBehavior: "contain" }}
      >
        <div className="flex h-full min-w-max gap-1.5">
        {COLUMNS.map((col) => {
          const cards = byColumn.get(col) ?? [];
          return (
            <div
              key={col}
              className="flex h-full shrink-0 flex-col overflow-hidden rounded border border-[var(--border)]/60 bg-[var(--surface)]/40"
              style={{ width: COL_W }}
            >
              <div className="shrink-0 px-1 py-1 text-center text-[9px] uppercase tracking-wide text-[var(--text-faint)]">
                {col.replace(/_/g, " ")}
                {cards.length > 0 ? ` · ${cards.length}` : ""}
              </div>
              <div
                data-goal-col-scroll
                className="nowheel h-0 min-h-0 flex-1 space-y-1 overflow-y-scroll overflow-x-hidden overscroll-contain px-1 pb-1"
              >
                {cards.map((t) => {
                  const kids = goalTaskChildren(tasks, t.id);
                  const doneKids = kids.filter((c) => c.column === "done").length;
                  return (
                    <button
                      key={t.id}
                      type="button"
                      onMouseDown={(e) => e.stopPropagation()}
                      onClick={() => setOpenTaskId(t.id)}
                      className="block w-full rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-1 text-left hover:border-violet-400/60"
                    >
                      <div className="flex items-center gap-1">
                        {t.kind && (
                          <span className={`text-[9px] ${KIND_COLOR[t.kind] ?? "text-gray-400"}`}>
                            {t.kind}
                          </span>
                        )}
                        <span className="min-w-0 flex-1 truncate text-[10px] text-gray-200">
                          {t.title}
                        </span>
                      </div>
                      {kids.length > 0 && (
                        <div className="mt-0.5 text-[9px] text-violet-300/80">
                          {doneKids}/{kids.length} sub-tasks
                        </div>
                      )}
                      {t.files && t.files.length > 0 && (
                        <div className="mt-0.5 truncate text-[9px] text-[var(--text-faint)]">
                          {t.files.join(", ")}
                        </div>
                      )}
                      {t.result && (
                        <div className="mt-0.5 truncate text-[9px] text-emerald-300/80">{t.result}</div>
                      )}
                      {t.error && (
                        <div className="mt-0.5 truncate text-[9px] text-red-300/80">{t.error}</div>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
        </div>
      </div>

      {openTaskId && (
        <GoalTaskModal tasks={tasks} taskId={openTaskId} onClose={() => setOpenTaskId(null)} />
      )}

      {/* Constraint memory strip */}
      {(memory.learned ?? []).length > 0 && (
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
