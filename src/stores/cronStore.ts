import { create } from "zustand";
import { api, type CronJob, type CronJobInput } from "../lib/tauri";

interface CronState {
  jobs: CronJob[];
  load: () => Promise<void>;
  save: (input: CronJobInput) => Promise<void>;
  remove: (id: string) => Promise<void>;
  runNow: (id: string) => Promise<void>;
}

export const useCronStore = create<CronState>((set, get) => ({
  jobs: [],
  load: async () => set({ jobs: await api.listCronJobs() }),
  save: async (input) => {
    await api.saveCronJob(input);
    await get().load();
  },
  remove: async (id) => {
    await api.deleteCronJob(id);
    await get().load();
  },
  runNow: async (id) => {
    await api.runCronJob(id);
    await get().load();
  },
}));
