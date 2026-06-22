import { create } from "zustand";
import { api, type InfraProject, type InfraProjectInput } from "../lib/tauri";

interface ProjectState {
  projects: InfraProject[];
  load: () => Promise<void>;
  save: (input: InfraProjectInput) => Promise<InfraProject>;
  remove: (id: string) => Promise<void>;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  load: async () => {
    const projects = await api.listInfraProjects();
    set({ projects });
  },
  save: async (input) => {
    const p = await api.saveInfraProject(input);
    await get().load();
    return p;
  },
  remove: async (id) => {
    await api.deleteInfraProject(id);
    await get().load();
  },
}));
