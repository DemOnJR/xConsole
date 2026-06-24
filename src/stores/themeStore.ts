import { create } from "zustand";
import { api } from "../lib/tauri";
import {
  applyTheme,
  buildCustomTheme,
  DEFAULT_THEME_ID,
  themeById,
  type Theme,
  type UiVars,
  type XtermColors,
} from "../lib/themes";

interface ThemeState {
  themeId: string;
  customVars: Partial<UiVars> | null;
  loaded: boolean;
  /** Load the persisted theme and apply it. Safe to call repeatedly. */
  load: () => Promise<void>;
  setTheme: (id: string) => Promise<void>;
  saveCustom: (vars: Partial<UiVars>) => Promise<void>;
  /** The resolved active theme (handles the custom slot). */
  current: () => Theme;
  /** The active terminal color scheme. */
  xterm: () => XtermColors;
}

export const useThemeStore = create<ThemeState>((set, get) => {
  const resolve = (themeId: string, customVars: Partial<UiVars> | null): Theme =>
    themeId === "custom" && customVars ? buildCustomTheme(customVars) : themeById(themeId);

  return {
    themeId: DEFAULT_THEME_ID,
    customVars: null,
    loaded: false,

    load: async () => {
      if (get().loaded) return;
      let themeId = DEFAULT_THEME_ID;
      let customVars: Partial<UiVars> | null = null;
      try {
        themeId = (await api.getSetting("ui.theme")) || DEFAULT_THEME_ID;
        const raw = await api.getSetting("ui.theme.custom");
        if (raw) customVars = JSON.parse(raw) as Partial<UiVars>;
      } catch {
        /* fresh install — defaults */
      }
      set({ themeId, customVars, loaded: true });
      applyTheme(resolve(themeId, customVars));
    },

    setTheme: async (id) => {
      set({ themeId: id });
      applyTheme(resolve(id, get().customVars));
      try {
        await api.setSetting("ui.theme", id);
      } catch {
        /* non-fatal */
      }
    },

    saveCustom: async (vars) => {
      set({ customVars: vars, themeId: "custom" });
      applyTheme(buildCustomTheme(vars));
      try {
        await api.setSetting("ui.theme.custom", JSON.stringify(vars));
        await api.setSetting("ui.theme", "custom");
      } catch {
        /* non-fatal */
      }
    },

    current: () => resolve(get().themeId, get().customVars),
    xterm: () => resolve(get().themeId, get().customVars).xterm,
  };
});
