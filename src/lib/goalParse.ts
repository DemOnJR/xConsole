import type { GoalMemory, GoalSession, GoalSpec, GoalTask } from "./tauri";

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

export function parseGoalTasks(raw: string | null | undefined): GoalTask[] {
  if (!raw || !raw.trim()) return [];
  try {
    const value = JSON.parse(raw) as unknown;
    if (!Array.isArray(value)) return [];
    return value
      .filter((item): item is Record<string, unknown> => !!item && typeof item === "object")
      .map((item, i) => ({
        id: typeof item.id === "string" && item.id ? item.id : `task-${i}`,
        column: typeof item.column === "string" ? item.column : "backlog",
        title: typeof item.title === "string" ? item.title : "",
        detail: typeof item.detail === "string" ? item.detail : null,
        kind: typeof item.kind === "string" ? item.kind : undefined,
        files: asStringArray(item.files),
        result: typeof item.result === "string" ? item.result : null,
        error: typeof item.error === "string" ? item.error : null,
        created_at: typeof item.created_at === "string" ? item.created_at : null,
        updated_at: typeof item.updated_at === "string" ? item.updated_at : null,
      }));
  } catch {
    return [];
  }
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
