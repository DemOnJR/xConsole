import { create } from "zustand";
import { persist } from "zustand/middleware";

/** App-level UI chrome state (modals/panels) kept in one place. */
interface UiState {
  settingsOpen: boolean;
  settingsSection: string;
  leftOpen: boolean;
  rightOpen: boolean;
  bottomOpen: boolean;
  agentOpen: boolean;
  agentExpanded: boolean;
  /** Console drawer expanded height (vs collapsed header only). */
  consoleExpanded: boolean;
  /** Broadcast keystrokes to all open console panes. */
  consoleBroadcast: boolean;

  openSettings: (section?: string) => void;
  closeSettings: () => void;
  setSettingsSection: (section: string) => void;
  toggleLeft: () => void;
  toggleRight: () => void;
  toggleBottom: () => void;
  toggleAgent: () => void;
  setAgentOpen: (open: boolean) => void;
  setAgentExpanded: (expanded: boolean) => void;
  toggleAgentExpanded: () => void;
  toggleConsoleExpanded: () => void;
  setConsoleBroadcast: (on: boolean) => void;
  toggleConsoleBroadcast: () => void;
}

type PersistedUi = Pick<
  UiState,
  | "leftOpen"
  | "rightOpen"
  | "bottomOpen"
  | "agentOpen"
  | "agentExpanded"
  | "consoleExpanded"
  | "consoleBroadcast"
  | "settingsSection"
>;

const PERSIST_DEFAULTS: PersistedUi = {
  leftOpen: true,
  rightOpen: true,
  bottomOpen: false,
  agentOpen: false,
  agentExpanded: false,
  consoleExpanded: true,
  consoleBroadcast: true,
  settingsSection: "providers",
};

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      settingsOpen: false,
      ...PERSIST_DEFAULTS,

      openSettings: (section) =>
        set((s) => ({
          settingsOpen: true,
          settingsSection: section ?? s.settingsSection,
        })),
      closeSettings: () => set({ settingsOpen: false }),
      setSettingsSection: (section) => set({ settingsSection: section }),
      toggleLeft: () => set((s) => ({ leftOpen: !s.leftOpen })),
      toggleRight: () => set((s) => ({ rightOpen: !s.rightOpen })),
      toggleBottom: () => set((s) => ({ bottomOpen: !s.bottomOpen })),
      toggleAgent: () =>
        set((s) => ({
          agentOpen: !s.agentOpen,
          agentExpanded: s.agentOpen ? false : s.agentExpanded,
        })),
      setAgentOpen: (open) =>
        set((s) => ({
          agentOpen: open,
          agentExpanded: open ? s.agentExpanded : false,
        })),
      setAgentExpanded: (expanded) => set({ agentExpanded: expanded }),
      toggleAgentExpanded: () =>
        set((s) => ({
          agentExpanded: s.agentOpen ? !s.agentExpanded : s.agentExpanded,
          agentOpen: true,
        })),
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
        agentOpen: state.agentOpen,
        agentExpanded: state.agentExpanded,
        consoleExpanded: state.consoleExpanded,
        consoleBroadcast: state.consoleBroadcast,
        settingsSection: state.settingsSection,
      }),
    },
  ),
);
