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
  /** Main view: the canvas workspace or the dedicated analytics page. */
  mainView: "canvas" | "analytics";
  rightOpen: boolean;
  /** Persisted width of the expanded workspace drawer. */
  leftWidth: number;
  /** Persisted width of the server drawer. */
  rightWidth: number;

  openSettings: (section?: string) => void;
  closeSettings: () => void;
  setSettingsSection: (section: string) => void;
  toggleLeft: () => void;
  toggleAnalytics: () => void;
  showCanvas: () => void;
  toggleRight: () => void;
  setLeftWidth: (width: number) => void;
  setRightWidth: (width: number) => void;
}

type PersistedUi = Pick<
  UiState,
  | "leftOpen"
  | "rightOpen"
  | "leftWidth"
  | "rightWidth"
  | "settingsSection"
>;

const PERSIST_DEFAULTS: PersistedUi = {
  leftOpen: true,
  rightOpen: true,
  leftWidth: DRAWER_WIDTH_DEFAULT,
  rightWidth: DRAWER_WIDTH_DEFAULT,
  settingsSection: "general",
};

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      settingsOpen: false,
      mainView: "canvas",
      ...PERSIST_DEFAULTS,

      openSettings: (section) =>
        set((s) => ({
          settingsOpen: true,
          settingsSection: section ?? s.settingsSection,
        })),
      closeSettings: () => set({ settingsOpen: false }),
      setSettingsSection: (section) => set({ settingsSection: section }),
      toggleLeft: () =>
        set((s) =>
          s.mainView === "analytics"
            ? { mainView: "canvas", leftOpen: true }
            : { leftOpen: !s.leftOpen },
        ),
      toggleAnalytics: () =>
        set((s) => ({
          mainView: s.mainView === "analytics" ? "canvas" : "analytics",
        })),
      showCanvas: () => set({ mainView: "canvas" }),
      toggleRight: () => set((s) => ({ rightOpen: !s.rightOpen })),
      setLeftWidth: (width) => set({ leftWidth: clampDrawerWidth(width) }),
      setRightWidth: (width) => set({ rightWidth: clampDrawerWidth(width) }),
    }),
    {
      name: "xconsole-ui",
      version: 1,
      partialize: (state): PersistedUi => ({
        leftOpen: state.leftOpen,
        rightOpen: state.rightOpen,
        leftWidth: state.leftWidth,
        rightWidth: state.rightWidth,
        settingsSection: state.settingsSection,
      }),
    },
  ),
);
