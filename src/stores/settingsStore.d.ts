import { type AiProvider, type AiProviderInput } from "../lib/tauri";
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
export declare const useSettingsStore: import("zustand").UseBoundStore<import("zustand").StoreApi<SettingsState>>;
export {};
//# sourceMappingURL=settingsStore.d.ts.map