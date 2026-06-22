import { useMemo } from "react";
import { SettingsIcon, TerminalIcon } from "../icons";
import type { AgentActivityItem } from "../../stores/agentStore";
import { CodeHighlight, ConsoleOutput, langFromPath, ShellCommand } from "./SyntaxHighlight";

function truncate(s: string, max: number): string {
  const flat = s.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  return `${flat.slice(0, max - 1)}…`;
}

/** Drop internal prefetch / status noise — only show user-meaningful tool steps. */
export function visibleActivityItems(items: AgentActivityItem[]): AgentActivityItem[] {
  const fileEditIds = new Set(items.filter((i) => i.kind === "file_edit").map((i) => i.id));
  return items.filter((item) => {
    if (!item.label.trim() && item.kind !== "file_edit") return false;
    if (item.kind === "status") return false;
    if (item.id.startsWith("snapshot-")) return false;
    if (item.kind === "tool" && fileEditIds.has(item.id)) return false;
    if (item.label === "SSH snapshot" || item.label === "Command output") return false;
    if (/^connecting to /i.test(item.label)) return false;
    if (/^starting cursor/i.test(item.label)) return false;
    if (/^launching `/i.test(item.label)) return false;
    if (item.label === "Working…" && !item.detail) return false;
    if (item.kind === "tool" && item.label.startsWith("Write file ·")) return false;
    return true;
  });
}

export function isCommandItem(item: AgentActivityItem): boolean {
  if (item.kind === "file_edit") return false;
  const raw = item.label.trim();
  if (item.kind === "command") return true;
  if (item.tool === "run_command" || item.tool === "shell") return true;
  if (raw.startsWith("SSH ›") || raw.startsWith("Shell ›")) return true;
  if (/^xconsole[-_]?run/i.test(raw)) return true;
  if (/^run command$/i.test(raw) && Boolean(item.detail)) return true;
  if (raw.startsWith("Run on ")) return true;
  return false;
}

function isMetaItem(item: AgentActivityItem): boolean {
  if (item.kind === "file_edit" || isCommandItem(item)) return false;
  const raw = item.label.trim();
  return (
    raw.startsWith("Read file ·") ||
    raw.startsWith("Read skill ·") ||
    raw.startsWith("Search ·") ||
    raw.startsWith("List ") ||
    item.kind === "skill_read" ||
    /^read /i.test(raw) ||
    /^grepped /i.test(raw) ||
    /^explored /i.test(raw)
  );
}

function metaLine(item: AgentActivityItem): string {
  const raw = item.label.trim();
  if (raw.startsWith("Read file ·")) {
    return `Read ${truncate(raw.slice("Read file ·".length).trim(), 72)}`;
  }
  if (raw.startsWith("Read skill ·")) {
    return `Read ${truncate(raw.slice("Read skill ·".length).trim(), 72)}`;
  }
  if (raw.startsWith("Search ·")) {
    return `Grepped ${truncate(raw.slice("Search ·".length).trim(), 72)}`;
  }
  if (item.kind === "skill_read" && item.category && item.name) {
    return `Read ${item.category}/${item.name}`;
  }
  return truncate(raw.replace(/^xconsole[-_\s]*/i, "").replace(/_/g, " "), 80);
}

function commandTitle(item: AgentActivityItem): string {
  const cmd =
    item.detail?.trim() ||
    item.label.replace(/^(SSH|Shell)\s*›\s*/i, "").trim() ||
    item.label.replace(/^Run on [^:]+:\s*/i, "").trim();
  const words = cmd.split(/\s+/).slice(0, 4).join(" ");
  return truncate(words, 48);
}

function commandBody(item: AgentActivityItem): string {
  return (
    item.detail?.trim() ||
    item.label.replace(/^(SSH|Shell)\s*›\s*/i, "").trim() ||
    item.label.replace(/^Run on [^:]+:\s*/i, "").trim() ||
    item.label
  );
}

function MetaLine({ text, dimmed }: { text: string; dimmed?: boolean }) {
  return (
    <div
      className={`text-[11px] leading-[1.35] ${dimmed ? "text-gray-600" : "text-gray-500"}`}
    >
      {text}
    </div>
  );
}

function FileEditCard({ item }: { item: AgentActivityItem }) {
  const running = item.state === "running";
  const fileName = item.path || item.label;
  const added = item.linesAdded ?? 0;
  const removed = item.linesRemoved ?? 0;
  const hunks = item.hunks ?? [];

  return (
    <div className="overflow-hidden rounded-lg border border-[#1f2737] bg-[#0d1118]">
      <div className="flex items-center gap-2 border-b border-[#1f2737]/80 px-2.5 py-1.5">
        <SettingsIcon size={12} className="shrink-0 text-gray-500" />
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-gray-300">
          {fileName}
        </span>
        {running ? (
          <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-gray-500" />
        ) : (
          <span className="flex shrink-0 items-center gap-1.5 font-mono text-[10px]">
            {added > 0 && <span className="text-emerald-400">+{added}</span>}
            {removed > 0 && <span className="text-red-400/90">-{removed}</span>}
          </span>
        )}
      </div>
      {hunks.length > 0 && (
        <div className="agent-activity-scroll max-h-[200px] overflow-y-auto text-[10px] leading-[1.45]">
          {hunks.map((h, i) => (
            <div
              key={i}
              className={`flex break-all px-2.5 py-px ${
                h.kind === "add"
                  ? "bg-emerald-950/50"
                  : h.kind === "del"
                    ? "bg-red-950/45"
                    : "bg-[#0a0e14]/80"
              }`}
            >
              <span
                className={`mr-1.5 select-none font-mono ${
                  h.kind === "add"
                    ? "text-emerald-500/80"
                    : h.kind === "del"
                      ? "text-red-500/80"
                      : "text-gray-600"
                }`}
              >
                {h.kind === "add" ? "+" : h.kind === "del" ? "-" : " "}
              </span>
              <CodeHighlight
                code={h.text}
                language={langFromPath(fileName)}
                className="inline text-[10px] text-gray-300"
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function CommandCard({ item }: { item: AgentActivityItem }) {
  const running = item.state === "running";
  const failed = item.state === "error";
  const cmd = commandBody(item);
  const output = item.output?.trim();

  return (
    <div
      className={`overflow-hidden rounded-lg border bg-[#0d1118] ${
        failed ? "border-red-900/50" : "border-[#1f2737]"
      }`}
    >
      <div className="flex items-center gap-2 border-b border-[#1f2737]/80 px-2.5 py-1.5">
        <TerminalIcon size={12} className="shrink-0 text-gray-500" />
        <span className="min-w-0 flex-1 truncate text-[11px] text-gray-400">
          {commandTitle(item)}
        </span>
        {running && (
          <span className="inline-block h-2 w-2 animate-spin rounded-full border border-gray-500 border-t-transparent" />
        )}
      </div>
      <div className="agent-activity-scroll max-h-[200px] overflow-y-auto px-2.5 py-2">
        <div className="flex gap-1.5">
          <span className="shrink-0 select-none font-mono text-[10px] text-emerald-500/90">$</span>
          <ShellCommand code={cmd} className="min-w-0 flex-1" />
        </div>
        {output && !running && (
          <div className="mt-2 border-t border-[#1f2737]/60 pt-2">
            <ConsoleOutput text={output} />
          </div>
        )}
      </div>
    </div>
  );
}

function ActivityBlock({ item }: { item: AgentActivityItem }) {
  if (item.kind === "file_edit") {
    return <FileEditCard item={item} />;
  }
  if (isCommandItem(item)) {
    return <CommandCard item={item} />;
  }
  if (isMetaItem(item)) {
    return <MetaLine text={metaLine(item)} />;
  }
  return <MetaLine text={metaLine(item)} dimmed={item.state === "running"} />;
}

export function AgentThinking() {
  return (
    <div className="flex items-center gap-2.5 px-1 py-1">
      <div className="flex gap-1">
        <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-gray-500 [animation-delay:0ms]" />
        <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-gray-500 [animation-delay:150ms]" />
        <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-gray-500 [animation-delay:300ms]" />
      </div>
      <span className="text-[11px] text-gray-500">Thinking…</span>
    </div>
  );
}

export function AgentActivityFeed({
  items,
  live = false,
}: {
  items: AgentActivityItem[];
  live?: boolean;
}) {
  const visible = useMemo(() => visibleActivityItems(items), [items]);

  if (visible.length === 0 && !live) return null;

  return (
    <div className="flex w-full flex-col gap-2">
      {visible.map((item) => (
        <ActivityBlock key={`${item.id}-${item.kind}`} item={item} />
      ))}
      {live && visible.length > 0 && (
        <MetaLine text="Planning next moves" dimmed />
      )}
    </div>
  );
}
