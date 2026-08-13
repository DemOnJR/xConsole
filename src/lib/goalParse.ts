import type { GoalMemory, GoalSession, GoalSpec, GoalTask, GoalTaskEvent } from "./tauri";

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

/** Empty intake `"{}"` is not a locked spec — return null so the UI stays quiet. */
export function parseGoalSpec(raw: string | null | undefined): GoalSpec | null {
  if (!raw || !raw.trim()) return null;
  try {
    const value = JSON.parse(raw) as Record<string, unknown>;
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const objective = typeof value.objective === "string" ? value.objective : "";
    const success_criteria = asStringArray(value.success_criteria);
    const check_method = typeof value.check_method === "string" ? value.check_method : "";
    if (!objective && success_criteria.length === 0 && !check_method) return null;
    return {
      objective,
      success_criteria,
      check_method,
      check_tooling: asStringArray(value.check_tooling),
      hard_constraints: asStringArray(value.hard_constraints),
      max_cycles: typeof value.max_cycles === "number" ? value.max_cycles : null,
    };
  } catch {
    return null;
  }
}

function parseGoalTaskEvents(raw: unknown): GoalTaskEvent[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((item): item is Record<string, unknown> => !!item && typeof item === "object")
    .map((item) => ({
      at: typeof item.at === "string" ? item.at : "",
      action: typeof item.action === "string" ? item.action : "updated",
      column: typeof item.column === "string" ? item.column : null,
      note: typeof item.note === "string" ? item.note : null,
    }));
}

function parseOneGoalTask(
  item: Record<string, unknown>,
  index: number,
  parentId: string | null,
): GoalTask {
  const id = typeof item.id === "string" && item.id ? item.id : `task-${index}`;
  const inherited =
    typeof item.parent_id === "string" && item.parent_id ? item.parent_id : parentId;
  return {
    id,
    column: typeof item.column === "string" ? item.column : "backlog",
    title: typeof item.title === "string" ? item.title : "",
    detail: typeof item.detail === "string" ? item.detail : null,
    kind: typeof item.kind === "string" ? item.kind : undefined,
    files: asStringArray(item.files),
    result: typeof item.result === "string" ? item.result : null,
    error: typeof item.error === "string" ? item.error : null,
    parent_id: inherited,
    history: parseGoalTaskEvents(item.history),
    created_at: typeof item.created_at === "string" ? item.created_at : null,
    updated_at: typeof item.updated_at === "string" ? item.updated_at : null,
  };
}

/** Flatten a kanban payload. Nested `subtasks` become cards with `parent_id`. */
function flattenGoalTasks(
  items: unknown[],
  parentId: string | null,
  start: number,
): GoalTask[] {
  const out: GoalTask[] = [];
  items.forEach((item) => {
    if (!item || typeof item !== "object" || Array.isArray(item)) return;
    const rec = item as Record<string, unknown>;
    const task = parseOneGoalTask(rec, start + out.length, parentId);
    out.push(task);
    if (Array.isArray(rec.subtasks)) {
      out.push(...flattenGoalTasks(rec.subtasks, task.id, start + out.length));
    }
  });
  return out;
}

export function parseGoalTasks(raw: string | null | undefined): GoalTask[] {
  if (!raw || !raw.trim()) return [];
  try {
    const value = JSON.parse(raw) as unknown;
    if (!Array.isArray(value)) return [];
    return flattenGoalTasks(value, null, 0);
  } catch {
    return [];
  }
}

/** Top-level cards (not a sub-task of another live card). */
export function goalRootTasks(tasks: GoalTask[]): GoalTask[] {
  const ids = new Set(tasks.map((t) => t.id));
  return tasks.filter((t) => !t.parent_id || !ids.has(t.parent_id));
}

export function goalTaskChildren(tasks: GoalTask[], parentId: string): GoalTask[] {
  return tasks.filter((t) => t.parent_id === parentId);
}

export function parseGoalMemory(raw: string | null | undefined): GoalMemory {
  try {
    const value = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
    const learnedRaw = Array.isArray(value?.learned) ? value.learned : [];
    const learned = learnedRaw
      .filter((item): item is Record<string, unknown> => !!item && typeof item === "object")
      .map((item) => ({
        key: String(item.key ?? ""),
        value: String(item.value ?? ""),
        evidence: String(item.evidence ?? ""),
        confidence: String(item.confidence ?? ""),
      }));
    return { learned };
  } catch {
    return { learned: [] };
  }
}

export function parseGoalSessionViews(session: GoalSession): {
  spec: GoalSpec | null;
  tasks: GoalTask[];
  memory: GoalMemory;
} {
  return {
    spec: parseGoalSpec(session.spec_json),
    tasks: parseGoalTasks(session.kanban_json),
    memory: parseGoalMemory(session.memory_json),
  };
}
