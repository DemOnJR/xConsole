export interface SlashCommandDef {
  name: string;
  description: string;
  syntax: string;
  actionKey: "new" | "clear" | "history" | "model" | "targets" | "plan" | "export" | "help";
}

export const SLASH_COMMANDS: SlashCommandDef[] = [
  {
    name: "new",
    syntax: "/new",
    description: "Start a fresh agent conversation",
    actionKey: "new",
  },
  {
    name: "clear",
    syntax: "/clear",
    description: "Clear the input composer",
    actionKey: "clear",
  },
  {
    name: "history",
    syntax: "/history",
    description: "Open conversation history and previous chats",
    actionKey: "history",
  },
  {
    name: "model",
    syntax: "/model",
    description: "Configure AI providers and select active model",
    actionKey: "model",
  },
  {
    name: "targets",
    syntax: "/targets",
    description: "Select target VPS hosts or switch to canvas scope",
    actionKey: "targets",
  },
  {
    name: "plan",
    syntax: "/plan",
    description: "Toggle Plan Mode (agent researches and proposes plan first)",
    actionKey: "plan",
  },
  {
    name: "export",
    syntax: "/export",
    description: "Export conversation transcript to Markdown",
    actionKey: "export",
  },
  {
    name: "help",
    syntax: "/help",
    description: "List all available slash commands and shortcuts",
    actionKey: "help",
  },
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
