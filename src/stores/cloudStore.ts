import { create } from "zustand";
import { api, type CloudAccount, type CloudAccountInput } from "../lib/tauri";

interface CloudState {
  accounts: CloudAccount[];
  load: () => Promise<void>;
  save: (input: CloudAccountInput) => Promise<CloudAccount>;
  remove: (id: string) => Promise<void>;
  scanResources: (id: string) => Promise<string>;
}

export const useCloudStore = create<CloudState>((set, get) => ({
  accounts: [],
  load: async () => {
    const accounts = await api.listCloudAccounts();
    set({ accounts });
  },
  save: async (input) => {
    const a = await api.saveCloudAccount(input);
    await get().load();
    return a;
  },
  remove: async (id) => {
    await api.deleteCloudAccount(id);
    await get().load();
  },
  scanResources: async (id) => api.listCloudResources(id, "all"),
}));
