import { create } from "zustand";
import { api, type GoalSession } from "../lib/tauri";
import { useWorkspaceStore } from "./workspaceStore";

interface GoalState {
  goals: GoalSession[];
  load: () => Promise<void>;
  start: (text: string) => Promise<string>;
  confirm: (id: string, targets?: string[]) => Promise<void>;
  pause: (id: string) => Promise<void>;
  resume: (id: string) => Promise<void>;
  stop: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  /** Apply a live event to a session (status change etc.). */
  patch: (id: string, partial: Partial<GoalSession>) => void;
}

export const useGoalStore = create<GoalState>((set, get) => ({
  goals: [],
  load: async () => set({ goals: await api.listGoals() }),
  start: async (text) => {
    // Filed under the project that is open, so the goal's agent gets that project's
    // brief and its messages stay out of every other project's thread.
    const id = await api.startGoal(text, useWorkspaceStore.getState().activeId);
    await get().load();
    return id;
  },
  confirm: async (id, targets) => {
    await api.confirmGoal(id, targets);
    await get().load();
  },
  pause: async (id) => {
    await api.pauseGoal(id);
    await get().load();
  },
  resume: async (id) => {
    await api.continueGoal(id);
    await get().load();
  },
  stop: async (id) => {
    await api.stopGoal(id);
    await get().load();
  },
  remove: async (id) => {
    await api.deleteGoal(id);
    await get().load();
  },
  patch: (id, partial) =>
    set((s) => ({
      goals: s.goals.map((g) => (g.id === id ? { ...g, ...partial } : g)),
    })),
}));
