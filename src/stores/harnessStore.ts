import { create } from "zustand";

export interface GoalTaskItem {
  id: string;
  text: string;
  done: boolean;
}

interface HarnessState {
  // Module Visibilities
  showGoal: boolean;
  goalCollapsed: boolean;
  showTools: boolean;
  toolsCollapsed: boolean;
  showContext: boolean;
  showLogs: boolean;
  layoutDensity: "compact" | "normal";

  // Goal Module Specific State
  activeGoal: string;
  goalStatus: "idle" | "running" | "paused" | "completed";
  goalTasks: GoalTaskItem[];

  // Actions
  toggleModule: (module: "goal" | "tools" | "context" | "logs") => void;
  toggleGoalCollapsed: () => void;
  toggleToolsCollapsed: () => void;
  setDensity: (density: "compact" | "normal") => void;
  setActiveGoal: (goal: string) => void;
  setGoalStatus: (status: "idle" | "running" | "paused" | "completed") => void;
  setGoalTasks: (tasks: GoalTaskItem[]) => void;
  toggleGoalTask: (id: string) => void;
  addGoalTask: (text: string) => void;
  removeGoalTask: (id: string) => void;
  clearGoal: () => void;
}

const STORAGE_KEY = "xconsole-agent-harness-settings";

function loadSavedSettings() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch {
    // fallback to defaults
  }
  return {};
}

function saveSettings(state: Partial<HarnessState>) {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        showGoal: state.showGoal,
        goalCollapsed: state.goalCollapsed,
        showTools: state.showTools,
        toolsCollapsed: state.toolsCollapsed,
        showContext: state.showContext,
        showLogs: state.showLogs,
        layoutDensity: state.layoutDensity,
      }),
    );
  } catch {
    // ignore
  }
}

const saved = loadSavedSettings();

export const useHarnessStore = create<HarnessState>((set, get) => ({
  showGoal: saved.showGoal ?? true,
  goalCollapsed: saved.goalCollapsed ?? false,
  showTools: saved.showTools ?? true,
  toolsCollapsed: saved.toolsCollapsed ?? false,
  showContext: saved.showContext ?? true,
  showLogs: saved.showLogs ?? false,
  layoutDensity: saved.layoutDensity ?? "compact",

  activeGoal: "",
  goalStatus: "idle",
  goalTasks: [],

  toggleModule: (module) => {
    set((s) => {
      const next = {
        ...s,
        showGoal: module === "goal" ? !s.showGoal : s.showGoal,
        showTools: module === "tools" ? !s.showTools : s.showTools,
        showContext: module === "context" ? !s.showContext : s.showContext,
        showLogs: module === "logs" ? !s.showLogs : s.showLogs,
      };
      saveSettings(next);
      return next;
    });
  },

  toggleGoalCollapsed: () =>
    set((s) => {
      const next = !s.goalCollapsed;
      saveSettings({ ...s, goalCollapsed: next });
      return { goalCollapsed: next };
    }),

  toggleToolsCollapsed: () =>
    set((s) => {
      const next = !s.toolsCollapsed;
      saveSettings({ ...s, toolsCollapsed: next });
      return { toolsCollapsed: next };
    }),

  setDensity: (density) => {
    set({ layoutDensity: density });
    saveSettings({ ...get(), layoutDensity: density });
  },

  setActiveGoal: (activeGoal) => set({ activeGoal, goalStatus: activeGoal.trim() ? "running" : "idle" }),

  setGoalStatus: (goalStatus) => set({ goalStatus }),

  setGoalTasks: (goalTasks) => set({ goalTasks }),

  toggleGoalTask: (id) =>
    set((s) => ({
      goalTasks: s.goalTasks.map((t) => (t.id === id ? { ...t, done: !t.done } : t)),
    })),

  addGoalTask: (text) =>
    set((s) => ({
      goalTasks: [...s.goalTasks, { id: `task-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`, text, done: false }],
    })),

  removeGoalTask: (id) =>
    set((s) => ({
      goalTasks: s.goalTasks.filter((t) => t.id !== id),
    })),

  clearGoal: () => set({ activeGoal: "", goalStatus: "idle", goalTasks: [] }),
}));
