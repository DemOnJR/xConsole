/** App-level UI chrome state (modals/panels) kept in one place. */
interface UiState {
    settingsOpen: boolean;
    settingsSection: string;
    leftOpen: boolean;
    /** Main view: the canvas workspace or the dedicated analytics page. */
    mainView: "canvas" | "analytics";
    rightOpen: boolean;
    bottomOpen: boolean;
    /** Persisted width of the expanded workspace drawer. */
    leftWidth: number;
    /** Persisted width of the server drawer. */
    rightWidth: number;
    /** Console drawer expanded height (vs collapsed header only). */
    consoleExpanded: boolean;
    /** Broadcast keystrokes to all open console panes. */
    consoleBroadcast: boolean;
    openSettings: (section?: string) => void;
    closeSettings: () => void;
    setSettingsSection: (section: string) => void;
    toggleLeft: () => void;
    toggleAnalytics: () => void;
    showCanvas: () => void;
    toggleRight: () => void;
    toggleBottom: () => void;
    setLeftWidth: (width: number) => void;
    setRightWidth: (width: number) => void;
    toggleConsoleExpanded: () => void;
    setConsoleBroadcast: (on: boolean) => void;
    toggleConsoleBroadcast: () => void;
}
type PersistedUi = Pick<UiState, "leftOpen" | "rightOpen" | "bottomOpen" | "leftWidth" | "rightWidth" | "consoleExpanded" | "consoleBroadcast" | "settingsSection">;
export declare const useUiStore: import("zustand").UseBoundStore<Omit<import("zustand").StoreApi<UiState>, "setState" | "persist"> & {
    setState(partial: UiState | Partial<UiState> | ((state: UiState) => UiState | Partial<UiState>), replace?: false | undefined): unknown;
    setState(state: UiState | ((state: UiState) => UiState), replace: true): unknown;
    persist: {
        setOptions: (options: Partial<import("zustand/middleware").PersistOptions<UiState, PersistedUi, unknown>>) => void;
        clearStorage: () => void;
        rehydrate: () => Promise<void> | void;
        hasHydrated: () => boolean;
        onHydrate: (fn: (state: UiState) => void) => () => void;
        onFinishHydration: (fn: (state: UiState) => void) => () => void;
        getOptions: () => Partial<import("zustand/middleware").PersistOptions<UiState, PersistedUi, unknown>>;
    };
}>;
export {};
//# sourceMappingURL=uiStore.d.ts.map