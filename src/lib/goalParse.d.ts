import type { GoalMemory, GoalSession, GoalSpec, GoalTask } from "./tauri";
/** Empty intake `"{}"` is not a locked spec — return null so the UI stays quiet. */
export declare function parseGoalSpec(raw: string | null | undefined): GoalSpec | null;
export declare function parseGoalTasks(raw: string | null | undefined): GoalTask[];
/** Top-level cards (not a sub-task of another live card). */
export declare function goalRootTasks(tasks: GoalTask[]): GoalTask[];
export declare function goalTaskChildren(tasks: GoalTask[], parentId: string): GoalTask[];
export declare function parseGoalMemory(raw: string | null | undefined): GoalMemory;
export declare function parseGoalSessionViews(session: GoalSession): {
    spec: GoalSpec | null;
    tasks: GoalTask[];
    memory: GoalMemory;
};
//# sourceMappingURL=goalParse.d.ts.map