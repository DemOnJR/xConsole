// App + terminal color themes. Each theme provides a set of semantic UI colors
// (written to CSS variables on <html>) and an xterm color scheme. The UI reads
// the variables via Tailwind `*-[var(--token)]` classes; terminals read `xterm`.

export interface XtermColors {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
}

/** Semantic UI tokens → CSS variable names. */
export const CSS_VARS = {
  bg: "--bg",
  surface: "--surface",
  surface2: "--surface-2",
  border: "--border",
  borderStrong: "--border-strong",
  text: "--text",
  textDim: "--text-dim",
  textFaint: "--text-faint",
  accent: "--accent",
  accentFg: "--accent-fg",
} as const;

export type UiVars = Record<keyof typeof CSS_VARS, string>;

export interface Theme {
  id: string;
  name: string;
  vars: UiVars;
  xterm: XtermColors;
}

// Tokyo Night — the app's original palette, kept as the default so nothing
// changes visually until the user picks another theme.
const TOKYO_NIGHT: Theme = {
  id: "tokyo-night",
  name: "Tokyo Night",
  vars: {
    bg: "#0b0f17",
    surface: "#11161f",
    surface2: "#0d121b",
    border: "#1f2737",
    borderStrong: "#2a3344",
    text: "#e5e7eb",
    textDim: "#9ca3af",
    textFaint: "#6b7280",
    accent: "#3b82f6",
    accentFg: "#ffffff",
  },
  xterm: {
    background: "#0b0f17",
    foreground: "#d6deeb",
    cursor: "#7aa2f7",
    selectionBackground: "#283457",
    black: "#1b1f2a",
    brightBlack: "#444b6a",
    red: "#f7768e",
    green: "#9ece6a",
    yellow: "#e0af68",
    blue: "#7aa2f7",
    magenta: "#bb9af7",
    cyan: "#7dcfff",
    white: "#a9b1d6",
  },
};

const CATPPUCCIN: Theme = {
  id: "catppuccin-mocha",
  name: "Catppuccin Mocha",
  vars: {
    bg: "#1e1e2e",
    surface: "#181825",
    surface2: "#11111b",
    border: "#313244",
    borderStrong: "#45475a",
    text: "#cdd6f4",
    textDim: "#a6adc8",
    textFaint: "#6c7086",
    accent: "#89b4fa",
    accentFg: "#11111b",
  },
  xterm: {
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    cursor: "#f5e0dc",
    selectionBackground: "#414458",
    black: "#45475a",
    brightBlack: "#585b70",
    red: "#f38ba8",
    green: "#a6e3a1",
    yellow: "#f9e2af",
    blue: "#89b4fa",
    magenta: "#cba6f7",
    cyan: "#94e2d5",
    white: "#bac2de",
  },
};

const DRACULA: Theme = {
  id: "dracula",
  name: "Dracula",
  vars: {
    bg: "#282a36",
    surface: "#21222c",
    surface2: "#191a21",
    border: "#44475a",
    borderStrong: "#6272a4",
    text: "#f8f8f2",
    textDim: "#c7c9d1",
    textFaint: "#6272a4",
    accent: "#bd93f9",
    accentFg: "#282a36",
  },
  xterm: {
    background: "#282a36",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    selectionBackground: "#44475a",
    black: "#21222c",
    brightBlack: "#6272a4",
    red: "#ff5555",
    green: "#50fa7b",
    yellow: "#f1fa8c",
    blue: "#bd93f9",
    magenta: "#ff79c6",
    cyan: "#8be9fd",
    white: "#f8f8f2",
  },
};

const GRUVBOX: Theme = {
  id: "gruvbox",
  name: "Gruvbox Dark",
  vars: {
    bg: "#282828",
    surface: "#32302f",
    surface2: "#1d2021",
    border: "#3c3836",
    borderStrong: "#504945",
    text: "#ebdbb2",
    textDim: "#a89984",
    textFaint: "#7c6f64",
    accent: "#fabd2f",
    accentFg: "#282828",
  },
  xterm: {
    background: "#282828",
    foreground: "#ebdbb2",
    cursor: "#ebdbb2",
    selectionBackground: "#504945",
    black: "#3c3836",
    brightBlack: "#665c54",
    red: "#fb4934",
    green: "#b8bb26",
    yellow: "#fabd2f",
    blue: "#83a598",
    magenta: "#d3869b",
    cyan: "#8ec07c",
    white: "#d5c4a1",
  },
};

const NORD: Theme = {
  id: "nord",
  name: "Nord",
  vars: {
    bg: "#2e3440",
    surface: "#3b4252",
    surface2: "#272c36",
    border: "#434c5e",
    borderStrong: "#4c566a",
    text: "#eceff4",
    textDim: "#d8dee9",
    textFaint: "#7b88a1",
    accent: "#88c0d0",
    accentFg: "#2e3440",
  },
  xterm: {
    background: "#2e3440",
    foreground: "#d8dee9",
    cursor: "#d8dee9",
    selectionBackground: "#434c5e",
    black: "#3b4252",
    brightBlack: "#4c566a",
    red: "#bf616a",
    green: "#a3be8c",
    yellow: "#ebcb8b",
    blue: "#81a1c1",
    magenta: "#b48ead",
    cyan: "#88c0d0",
    white: "#e5e9f0",
  },
};

const ONE_DARK: Theme = {
  id: "one-dark",
  name: "One Dark",
  vars: {
    bg: "#282c34",
    surface: "#21252b",
    surface2: "#1b1f23",
    border: "#3b4048",
    borderStrong: "#4b5263",
    text: "#abb2bf",
    textDim: "#828997",
    textFaint: "#5c6370",
    accent: "#61afef",
    accentFg: "#282c34",
  },
  xterm: {
    background: "#282c34",
    foreground: "#abb2bf",
    cursor: "#61afef",
    selectionBackground: "#3e4451",
    black: "#3f4451",
    brightBlack: "#5c6370",
    red: "#e06c75",
    green: "#98c379",
    yellow: "#e5c07b",
    blue: "#61afef",
    magenta: "#c678dd",
    cyan: "#56b6c2",
    white: "#abb2bf",
  },
};

const SOLARIZED: Theme = {
  id: "solarized-dark",
  name: "Solarized Dark",
  vars: {
    bg: "#002b36",
    surface: "#073642",
    surface2: "#00252e",
    border: "#0a4a59",
    borderStrong: "#586e75",
    text: "#93a1a1",
    textDim: "#839496",
    textFaint: "#657b83",
    accent: "#268bd2",
    accentFg: "#fdf6e3",
  },
  xterm: {
    background: "#002b36",
    foreground: "#93a1a1",
    cursor: "#93a1a1",
    selectionBackground: "#073642",
    black: "#073642",
    brightBlack: "#586e75",
    red: "#dc322f",
    green: "#859900",
    yellow: "#b58900",
    blue: "#268bd2",
    magenta: "#d33682",
    cyan: "#2aa198",
    white: "#eee8d5",
  },
};

export const THEMES: Theme[] = [
  TOKYO_NIGHT,
  CATPPUCCIN,
  DRACULA,
  GRUVBOX,
  NORD,
  ONE_DARK,
  SOLARIZED,
];

export const DEFAULT_THEME_ID = TOKYO_NIGHT.id;

export function themeById(id: string | null | undefined): Theme {
  return THEMES.find((t) => t.id === id) ?? TOKYO_NIGHT;
}

/** Build a custom theme from a partial set of base colors (the rest derive from defaults). */
export function buildCustomTheme(vars: Partial<UiVars>): Theme {
  return {
    id: "custom",
    name: "Custom",
    vars: { ...TOKYO_NIGHT.vars, ...vars },
    // Custom themes reuse Tokyo Night's terminal palette unless overridden later.
    xterm: { ...TOKYO_NIGHT.xterm, background: vars.bg ?? TOKYO_NIGHT.xterm.background },
  };
}

/** Write a theme's UI variables to <html> so every `var(--token)` updates live. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  (Object.keys(CSS_VARS) as (keyof typeof CSS_VARS)[]).forEach((k) => {
    root.style.setProperty(CSS_VARS[k], theme.vars[k]);
  });
  root.setAttribute("data-theme", theme.id);
}
