import { create } from "zustand";
import { api, type GoalSession } from "../lib/tauri";

interface GoalState {
  goals: GoalSession[];
  load: () => Promise<void>;
  start: (text: string) => Promise<string>;
  confirm: (id: string) => Promise<void>;
  stop: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  /** Apply a live event to a session (status change etc.). */
  patch: (id: string, partial: Partial<GoalSession>) => void;
}

export const useGoalStore = create<GoalState>((set, get) => ({
  goals: [],
  load: async () => set({ goals: await api.listGoals() }),
  start: async (text) => {
    const id = await api.startGoal(text);
    await get().load();
    return id;
  },
  confirm: async (id) => {
    await api.confirmGoal(id);
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
