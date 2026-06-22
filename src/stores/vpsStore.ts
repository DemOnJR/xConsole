import { create } from "zustand";
import { api, type Vps, type VpsInput } from "../lib/tauri";

interface VpsState {
  vpsList: Vps[];
  loading: boolean;
  load: () => Promise<void>;
  save: (input: VpsInput) => Promise<Vps>;
  remove: (id: string) => Promise<void>;
}

export const useVpsStore = create<VpsState>((set, get) => ({
  vpsList: [],
  loading: false,

  load: async () => {
    set({ loading: true });
    try {
      const vpsList = await api.listVps();
      set({ vpsList });
    } finally {
      set({ loading: false });
    }
  },

  save: async (input) => {
    const saved = await api.saveVps(input);
    await get().load();
    return saved;
  },

  remove: async (id) => {
    await api.deleteVps(id);
    await get().load();
  },
}));
