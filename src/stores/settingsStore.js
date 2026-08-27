import { create } from "zustand";
import { api } from "../lib/tauri";
export const useSettingsStore = create((set, get) => ({
    settings: {},
    providers: [],
    loaded: false,
    load: async () => {
        const [rows, providers] = await Promise.all([
            api.listSettings(),
            api.listProviders(),
        ]);
        const settings = {};
        for (const r of rows)
            settings[r.key] = r.value;
        set({ settings, providers, loaded: true });
    },
    get: (key, fallback) => get().settings[key] ?? fallback,
    set: async (key, value) => {
        await api.setSetting(key, value);
        set((s) => ({ settings: { ...s.settings, [key]: value } }));
    },
    loadProviders: async () => {
        const providers = await api.listProviders();
        set({ providers });
    },
    saveProvider: async (input) => {
        const saved = await api.saveProvider(input);
        await get().loadProviders();
        return saved;
    },
    removeProvider: async (id) => {
        await api.deleteProvider(id);
        await get().loadProviders();
    },
}));
//# sourceMappingURL=settingsStore.js.map