import { create } from "zustand";
import { persist } from "zustand/middleware";
import {
  clampDrawerWidth,
  DRAWER_WIDTH_DEFAULT,
} from "../lib/uiLayout";

/** App-level UI chrome state (modals/panels) kept in one place. */
interface UiState {
  settingsOpen: boolean;
  settingsSection: string;
  leftOpen: boolean;
  /** Which page the left drawer is showing. */
  leftMode: "workspaces" | "analytics";
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
  openAnalytics: () => void;
  toggleRight: () => void;
  toggleBottom: () => void;
  setLeftWidth: (width: number) => void;
  setRightWidth: (width: number) => void;
  toggleConsoleExpanded: () => void;
  setConsoleBroadcast: (on: boolean) => void;
  toggleConsoleBroadcast: () => void;
}

type PersistedUi = Pick<
  UiState,
  | "leftOpen"
  | "rightOpen"
  | "bottomOpen"
  | "leftWidth"
  | "rightWidth"
  | "consoleExpanded"
  | "consoleBroadcast"
  | "settingsSection"
>;

const PERSIST_DEFAULTS: PersistedUi = {
  leftOpen: true,
  rightOpen: true,
  bottomOpen: false,
  leftWidth: DRAWER_WIDTH_DEFAULT,
  rightWidth: DRAWER_WIDTH_DEFAULT,
  consoleExpanded: true,
  consoleBroadcast: true,
  settingsSection: "general",
};

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      settingsOpen: false,
      leftMode: "workspaces",
      ...PERSIST_DEFAULTS,

      openSettings: (section) =>
        set((s) => ({
          settingsOpen: true,
          settingsSection: section ?? s.settingsSection,
        })),
      closeSettings: () => set({ settingsOpen: false }),
      setSettingsSection: (section) => set({ settingsSection: section }),
      toggleLeft: () =>
        set((s) => ({
          leftOpen: s.leftMode === "workspaces" ? !s.leftOpen : true,
          leftMode: "workspaces",
        })),
      openAnalytics: () =>
        set((s) => ({
          leftOpen: s.leftMode === "analytics" ? !s.leftOpen : true,
          leftMode: "analytics",
        })),
      toggleRight: () => set((s) => ({ rightOpen: !s.rightOpen })),
      toggleBottom: () => set((s) => ({ bottomOpen: !s.bottomOpen })),
      setLeftWidth: (width) => set({ leftWidth: clampDrawerWidth(width) }),
      setRightWidth: (width) => set({ rightWidth: clampDrawerWidth(width) }),
      toggleConsoleExpanded: () =>
        set((s) => ({ consoleExpanded: !s.consoleExpanded })),
      setConsoleBroadcast: (on) => set({ consoleBroadcast: on }),
      toggleConsoleBroadcast: () =>
        set((s) => ({ consoleBroadcast: !s.consoleBroadcast })),
    }),
    {
      name: "xconsole-ui",
      version: 1,
      partialize: (state): PersistedUi => ({
        leftOpen: state.leftOpen,
        rightOpen: state.rightOpen,
        bottomOpen: state.bottomOpen,
        leftWidth: state.leftWidth,
        rightWidth: state.rightWidth,
        consoleExpanded: state.consoleExpanded,
        consoleBroadcast: state.consoleBroadcast,
        settingsSection: state.settingsSection,
      }),
    },
  ),
);
