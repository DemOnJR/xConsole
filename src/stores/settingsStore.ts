import { create } from "zustand";
import { api, type AiProvider, type AiProviderInput } from "../lib/tauri";

interface SettingsState {
  /** In-memory cache of every key/value setting. */
  settings: Record<string, string>;
  providers: AiProvider[];
  loaded: boolean;

  load: () => Promise<void>;

  get: (key: string, fallback?: string) => string | undefined;
  set: (key: string, value: string) => Promise<void>;

  loadProviders: () => Promise<void>;
  saveProvider: (input: AiProviderInput) => Promise<AiProvider>;
  removeProvider: (id: string) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: {},
  providers: [],
  loaded: false,

  load: async () => {
    const [rows, providers] = await Promise.all([
      api.listSettings(),
      api.listProviders(),
    ]);
    const settings: Record<string, string> = {};
    for (const r of rows) settings[r.key] = r.value;
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
