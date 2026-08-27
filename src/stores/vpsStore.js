import { create } from "zustand";
import { api } from "../lib/tauri";
export const useVpsStore = create((set, get) => ({
    vpsList: [],
    loading: false,
    load: async () => {
        set({ loading: true });
        try {
            const vpsList = await api.listVps();
            set({ vpsList });
        }
        finally {
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
    reorder: async (srcId, targetId) => {
        if (srcId === targetId)
            return;
        const list = [...get().vpsList];
        const from = list.findIndex((v) => v.id === srcId);
        const to = list.findIndex((v) => v.id === targetId);
        if (from < 0 || to < 0)
            return;
        const [moved] = list.splice(from, 1);
        list.splice(to, 0, moved);
        set({ vpsList: list }); // optimistic
        try {
            await api.reorderVps(list.map((v) => v.id));
        }
        catch {
            await get().load(); // revert to persisted order on failure
        }
    },
}));
//# sourceMappingURL=vpsStore.js.map