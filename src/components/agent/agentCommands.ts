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
    | "conversation"
    | "loop"
    | "goal"
    | "vision"
    | "safety"
    | "reasoning";
  /** When set, picking the command inserts `/name ` so the user can type the rest. */
  needsArg?: boolean;
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
    name: "safety",
    syntax: "/safety",
    description: "Set safety mode: full / allowlist / approve",
    actionKey: "safety",
  },
  {
    name: "reasoning",
    syntax: "/reasoning",
    description: "Set reasoning effort: off / low / medium / high",
    actionKey: "reasoning",
  },
  {
    name: "think",
    syntax: "/think",
    description: "Set reasoning effort (thinking budget): off / low / medium / high",
    actionKey: "reasoning",
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
    name: "loop",
    syntax: "/loop <task>",
    description: "Loop the task until the agent finishes (Esc to stop)",
    actionKey: "loop",
    needsArg: true,
  },
  {
    name: "goal",
    syntax: "/goal <objective>",
    description: "Set an autonomous goal — the agent asks what it needs once, then works until it's done",
    actionKey: "goal",
    needsArg: true,
  },
  {
    name: "vision",
    syntax: "/vision",
    description: "Image vision: on/ask/off and pick the vision model (Gemini recommended)",
    actionKey: "vision",
  },
  {
    name: "help",
    syntax: "/help",
    description: "List all available slash commands and shortcuts",
    actionKey: "help",
  },
];

/** Keybinds shown in /help. */
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

/** First token after `/` and anything after the first space. */
export function slashQuery(input: string): { name: string; rest: string } | null {
  const trimmed = input.trimStart();
  if (!trimmed.startsWith("/")) return null;
  const body = trimmed.slice(1);
  const sp = body.search(/\s/);
  if (sp < 0) return { name: body.toLowerCase(), rest: "" };
  return { name: body.slice(0, sp).toLowerCase(), rest: body.slice(sp + 1).trim() };
}

export function filterSlashCommands(input: string): SlashCommandDef[] {
  const q = slashQuery(input);
  if (!q) return [];
  // `/goal deploy nginx` is an invocation, not a menu pick — don't steal Enter.
  if (q.rest) return [];
  // `/goal ` (trailing space) means the user is about to type the objective.
  if (q.name && /\s$/.test(input) && SLASH_COMMANDS.some((c) => c.name === q.name)) {
    return [];
  }
  if (!q.name) return SLASH_COMMANDS;
  return SLASH_COMMANDS.filter(
    (cmd) =>
      cmd.name.toLowerCase().includes(q.name) ||
      cmd.description.toLowerCase().includes(q.name),
  );
}

export function parseExactSlashCommand(input: string): SlashCommandDef | null {
  const q = slashQuery(input);
  if (!q || q.rest) return null;
  return SLASH_COMMANDS.find((cmd) => cmd.name === q.name) ?? null;
}
