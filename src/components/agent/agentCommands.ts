export interface SlashCommandDef {
  name: string;
  description: string;
  syntax: string;
  actionKey:
    | "new"
    | "clear"
    | "history"
    | "model"
    | "targets"
    | "plan"
    | "export"
    | "compact"
    | "help"
    | "ctx"
    | "cost"
    | "voice"
    | "conversation";
}

export const SLASH_COMMANDS: SlashCommandDef[] = [
  {
    name: "model",
    syntax: "/model",
    description: "Pick the active AI provider (arrows + Enter)",
    actionKey: "model",
  },
  {
    name: "targets",
    syntax: "/targets",
    description: "Select target VPS hosts (space toggles, enter done)",
    actionKey: "targets",
  },
  {
    name: "new",
    syntax: "/new",
    description: "Start a fresh agent conversation",
    actionKey: "new",
  },
  {
    name: "clear",
    syntax: "/clear",
    description: "Clear the input line",
    actionKey: "clear",
  },
  {
    name: "history",
    syntax: "/history",
    description: "Browse past conversations (arrows + Enter)",
    actionKey: "history",
  },
  {
    name: "plan",
    syntax: "/plan",
    description: "Toggle Plan Mode (Shift+Tab)",
    actionKey: "plan",
  },
  {
    name: "export",
    syntax: "/export",
    description: "Export conversation transcript to Markdown",
    actionKey: "export",
  },
  {
    name: "compact",
    syntax: "/compact",
    description: "Compact context window and summarize earlier conversation",
    actionKey: "compact",
  },
  {
    name: "ctx",
    syntax: "/ctx",
    description: "Show context usage breakdown",
    actionKey: "ctx",
  },
  {
    name: "cost",
    syntax: "/cost",
    description: "Show running conversation cost",
    actionKey: "cost",
  },
  {
    name: "voice",
    syntax: "/voice",
    description: "Toggle spoken replies (TTS)",
    actionKey: "voice",
  },
  {
    name: "conversation",
    syntax: "/conversation",
    description: "Hands-free conversation mode (listen continuously)",
    actionKey: "conversation",
  },
  {
    name: "help",
    syntax: "/help",
    description: "List all available slash commands and shortcuts",
    actionKey: "help",
  },
];

/** Claude Code-style keybinds shown in /help. */
export const KEYBINDS: { keys: string; action: string }[] = [
  { keys: "Ctrl+K", action: "command palette (/)" },
  { keys: "Ctrl+L", action: "clear input" },
  { keys: "Ctrl+Z / Ctrl+Y", action: "undo / redo" },
  { keys: "Ctrl+R", action: "cycle provider" },
  { keys: "Shift+Tab", action: "toggle plan mode" },
  { keys: "↑ / ↓", action: "recall previous input" },
  { keys: "Tab", action: "complete slash command" },
  { keys: "Esc", action: "stop agent / close picker" },
];

export function isSlashInput(input: string): boolean {
  return input.trimStart().startsWith("/");
}

export function filterSlashCommands(input: string): SlashCommandDef[] {
  const trimmed = input.trimStart();
  if (!trimmed.startsWith("/")) return [];
  const query = trimmed.slice(1).trim().toLowerCase();
  if (!query) return SLASH_COMMANDS;
  return SLASH_COMMANDS.filter(
    (cmd) =>
      cmd.name.toLowerCase().includes(query) ||
      cmd.description.toLowerCase().includes(query),
  );
}

export function parseExactSlashCommand(input: string): SlashCommandDef | null {
  const trimmed = input.trim().toLowerCase();
  if (!trimmed.startsWith("/")) return null;
  const name = trimmed.slice(1);
  return SLASH_COMMANDS.find((cmd) => cmd.name === name) ?? null;
}
