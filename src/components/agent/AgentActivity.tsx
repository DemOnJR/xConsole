import { memo, useEffect, useMemo, useState } from "react";
import { useAgentStore, type AgentActivityItem } from "../../stores/agentStore";
import type { DiffLine } from "../../lib/tauri";
import { CodeHighlight, ConsoleOutput, langFromPath, ShellCommand } from "./SyntaxHighlight";
import { useVpsStore } from "../../stores/vpsStore";
import { useCanvasStore } from "../../stores/canvasStore";
import { redactExportText } from "../../lib/agentExport";
import { HashSpinner } from "./HashSpinner";
import { useMaskHost } from "../../lib/privacy";

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
    // Parallel-batch stays in the feed. Cache hit/miss lives on the input bar.
    if (item.kind === "status") {
      if (item.id.startsWith("cache-") || /^cache /i.test(item.label)) return false;
      return item.id === "parallel-batch" || /parallel/i.test(item.label);
    }
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

export function isTodoItem(item: AgentActivityItem): boolean {
  return (
    item.tool === "todo_write" ||
    /^update checklist$/i.test(item.label.trim()) ||
    /^todo write$/i.test(item.label.trim())
  );
}

export function isFileEditItem(item: AgentActivityItem): boolean {
  if (item.kind === "file_edit") return true;
  const tool = (item.tool || "").toLowerCase();
  if (tool === "edit_file" || tool === "local_edit_file" || tool === "write_file" || tool === "local_write_file") {
    return true;
  }
  return Boolean(item.hunks && item.hunks.length > 0);
}

export function isFileReadItem(item: AgentActivityItem): boolean {
  if (isFileEditItem(item) || isCommandItem(item) || isTodoItem(item)) return false;
  const tool = (item.tool || "").toLowerCase();
  const raw = item.label.trim();
  return (
    tool === "read_file" ||
    tool === "local_read_file" ||
    raw.startsWith("Read file ·") ||
    /^read /i.test(raw) ||
    tool === "local_list_dir" ||
    raw.startsWith("List ")
  );
}

export function isSearchItem(item: AgentActivityItem): boolean {
  if (isFileEditItem(item) || isCommandItem(item) || isTodoItem(item)) return false;
  const tool = (item.tool || "").toLowerCase();
  const raw = item.label.trim();
  return (
    tool === "grep_search" ||
    tool === "local_grep_search" ||
    raw.startsWith("Search ·") ||
    /^search/i.test(raw) ||
    /^grepped /i.test(raw)
  );
}

export function isMetaItem(item: AgentActivityItem): boolean {
  if (isFileEditItem(item) || isCommandItem(item) || isTodoItem(item)) return false;
  const raw = item.label.trim();
  return (
    isFileReadItem(item) ||
    isSearchItem(item) ||
    raw.startsWith("Read skill ·") ||
    item.kind === "skill_read" ||
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
  const cmd = redactExportText(
    item.detail?.trim() ||
      item.label.replace(/^(SSH|Shell)\s*›\s*/i, "").trim() ||
      item.label.replace(/^Run on [^:]+:\s*/i, "").trim(),
  );
  const words = cmd.split(/\s+/).slice(0, 5).join(" ");
  return truncate(words, 56);
}

function commandBody(item: AgentActivityItem): string {
  return (
    item.detail?.trim() ||
    item.label.replace(/^(SSH|Shell)\s*›\s*/i, "").trim() ||
    item.label.replace(/^Run on [^:]+:\s*/i, "").trim() ||
    item.label
  );
}

/** Extract host label from "Run on <name>" or "Edit ... on <name>" activity titles. */
function hostFromCommandLabel(label: string): string | null {
  const m = /on\s+([A-Za-z0-9_.-]+(?:\s*\([^)]+\))?)$/i.exec(label.trim());
  if (m?.[1]) return m[1].trim();
  const m2 = /^Run on (.+)$/i.exec(label.trim());
  return m2?.[1]?.trim() || null;
}

export function liveGerund(item: AgentActivityItem): string {
  const tool = (item.tool || "").toLowerCase();
  const label = item.label.trim();
  const path = item.path || "";
  if (item.kind === "file_edit" || tool === "write_file" || /^write /i.test(label)) {
    return `Writing ${truncate(path || label.replace(/^Write( file)? ·\s*/i, ""), 56)}`;
  }
  if (tool === "read_file" || /^read /i.test(label) || label.startsWith("Read file")) {
    return `Reading ${truncate(path || label.replace(/^Read( file)? ·\s*/i, ""), 56)}`;
  }
  if (tool === "terminal_send") {
    return `Typing in terminal ${truncate(item.detail || label.replace(/^Type in live terminal:\s*/i, ""), 48)}`;
  }
  if (tool === "terminal_capture") return "Reading live terminal";
  if (tool === "grep_search" || tool === "local_grep_search") {
    return `Searching ${truncate(item.detail || item.label.replace(/^Search\s+/i, ""), 48)}`;
  }
  if (tool === "edit_file" || tool === "local_edit_file") {
    return `Editing ${truncate(path || label.replace(/^Edit\s+/i, ""), 56)}`;
  }
  if (tool === "todo_write") return "Updating checklist";
  if (tool === "canvas_open_terminal") return "Opening terminal";
  if (tool === "canvas_refresh") return "Reconnecting terminal";
  if (isCommandItem(item)) {
    const host = hostFromCommandLabel(label);
    const cmd = commandTitle(item);
    return host ? `Executing ${cmd} on ${host}` : `Executing ${cmd}`;
  }
  if (label) return truncate(label, 72);
  return "Working";
}

export function activitySummary(items: AgentActivityItem[]): string {
  const visible = visibleActivityItems(items);
  let commands = 0;
  let reads = 0;
  let writes = 0;
  for (const item of visible) {
    if (isCommandItem(item)) commands += 1;
    else if (isFileEditItem(item)) writes += 1;
    else if (isFileReadItem(item) || isSearchItem(item)) reads += 1;
  }
  const parts: string[] = [];
  if (commands) parts.push(`executed ${commands} command${commands === 1 ? "" : "s"}`);
  if (reads) parts.push(`read ${reads} file${reads === 1 ? "" : "s"}`);
  if (writes) parts.push(`wrote ${writes} file${writes === 1 ? "" : "s"}`);
  if (parts.length === 0) return `${visible.length} step${visible.length === 1 ? "" : "s"}`;
  return parts.join(" · ");
}

/** Computes total diff additions & deletions across all file edits in this turn */
function totalDiffStats(items: AgentActivityItem[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const item of items) {
    if (item.linesAdded) added += item.linesAdded;
    if (item.linesRemoved) removed += item.linesRemoved;
    if (item.hunks) {
      for (const h of item.hunks) {
        if (h.kind === "add") added += 1;
        else if (h.kind === "del") removed += 1;
      }
    }
  }
  return { added, removed };
}

function DiffViewer({ hunks, fileName }: { hunks: DiffLine[]; fileName: string }) {
  const maskHost = useMaskHost();
  const lang = langFromPath(fileName);
  if (hunks.length === 0) return null;
  return (
    <div className="agent-activity-scroll my-1.5 max-h-[320px] overflow-y-auto rounded border border-[var(--border)] bg-[#070a0f] font-mono text-[11px] leading-[1.5]">
      {hunks.map((h, i) => {
        const isAdd = h.kind === "add";
        const isDel = h.kind === "del";
        return (
          <div
            key={i}
            className={`flex items-start px-2 py-0.5 font-mono ${
              isAdd
                ? "bg-emerald-950/40 text-emerald-200 border-l-2 border-emerald-500"
                : isDel
                  ? "bg-red-950/40 text-red-200 border-l-2 border-red-500"
                  : "bg-transparent text-gray-400"
            }`}
          >
            <span
              className={`mr-2 shrink-0 select-none font-bold ${
                isAdd ? "text-emerald-400" : isDel ? "text-red-400" : "text-gray-600"
              }`}
            >
              {isAdd ? "+" : isDel ? "-" : " "}
            </span>
            <CodeHighlight
              code={maskHost(h.text)}
              language={lang}
              className="inline min-w-0 flex-1 break-all whitespace-pre-wrap text-[11px]"
            />
          </div>
        );
      })}
    </div>
  );
}

/** Individual tool execution row inside the expanded activity feed (Cursor / Claude Code style). */
function ActivityItemRow({
  item,
  open,
  onToggle,
}: {
  item: AgentActivityItem;
  open: boolean;
  onToggle: () => void;
}) {
  const maskHost = useMaskHost();
  const running = item.state === "running";
  const failed = item.state === "error";
  const hostLabel = hostFromCommandLabel(item.label);
  const isCmd = isCommandItem(item);
  const isEdit = isFileEditItem(item);
  const isRead = isFileReadItem(item);
  const isSearch = isSearchItem(item);

  const cmd = redactExportText(commandBody(item));
  const output = item.output?.trim();
  const hunks = item.hunks ?? [];
  const added = item.linesAdded ?? hunks.filter((h) => h.kind === "add").length;
  const removed = item.linesRemoved ?? hunks.filter((h) => h.kind === "del").length;
  const fileName = item.path || item.label.replace(/^Edit\s+/i, "").replace(/^Write\s+/i, "").trim();

  const vpsList = useVpsStore((s) => s.vpsList);
  const addVps = useCanvasStore((s) => s.addVps);
  const focus = useCanvasStore((s) => s.focus);
  const nodes = useCanvasStore((s) => s.nodes);

  const openOnCanvas = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!hostLabel) return;
    const vps = vpsList.find(
      (v) => v.name === hostLabel || v.host === hostLabel || v.id === hostLabel,
    );
    if (!vps) return;
    const existing = nodes.find(
      (n) => n.type === "terminal" && String(n.data.vpsId) === vps.id,
    );
    if (existing) focus(existing.id);
    else {
      const id = addVps(vps);
      focus(id);
    }
  };

  const copyOutput = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (output) void navigator.clipboard.writeText(output);
  };

  const copyCommand = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (cmd) void navigator.clipboard.writeText(cmd);
  };

  // Icon badge based on tool category
  const renderBadge = () => {
    if (running) {
      return (
        <span
          className="inline-block h-3 w-3 shrink-0 animate-spin rounded-full border border-cyan-400 border-t-transparent"
          aria-label="Running"
        />
      );
    }
    if (failed) {
      return (
        <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-red-950/60 font-mono text-[10px] font-bold text-red-400 border border-red-500/30">
          ✕
        </span>
      );
    }
    if (isCmd) {
      return (
        <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-emerald-950/60 font-mono text-[10px] font-bold text-emerald-400 border border-emerald-500/30">
          $
        </span>
      );
    }
    if (isEdit) {
      return (
        <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-sky-950/60 font-mono text-[10px] font-bold text-sky-400 border border-sky-500/30">
          ✎
        </span>
      );
    }
    if (isRead) {
      return (
        <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-slate-800/60 text-[10px] text-slate-300 border border-slate-600/30">
          📄
        </span>
      );
    }
    if (isSearch) {
      return (
        <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-cyan-950/60 text-[10px] text-cyan-300 border border-cyan-500/30">
          🔍
        </span>
      );
    }
    return (
      <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-purple-950/60 text-[10px] text-purple-300 border border-purple-500/30">
        ⚡
      </span>
    );
  };

  // Row label text
  const labelText = () => {
    if (isCmd) return cmd || commandTitle(item);
    if (isEdit) return `Edited ${fileName}`;
    if (isRead) return metaLine(item);
    if (isSearch) return metaLine(item);
    return item.label;
  };

  const hasExpandableContent = Boolean(output || (hunks && hunks.length > 0) || item.detail);

  return (
    <div className="flex flex-col border-b border-[var(--border)]/40 last:border-b-0">
      {/* Clickable summary line */}
      <div
        onClick={() => hasExpandableContent && onToggle()}
        className={`group flex items-center justify-between gap-2 px-2.5 py-1.5 transition select-none ${
          hasExpandableContent ? "cursor-pointer hover:bg-[var(--surface-hover)]" : ""
        } ${open ? "bg-[var(--surface-hover)]/70" : ""}`}
      >
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {renderBadge()}

          <span
            className={`min-w-0 flex-1 truncate font-mono text-[11px] ${
              failed
                ? "text-red-300 font-medium"
                : running
                  ? "text-cyan-200 font-medium"
                  : "text-gray-300 group-hover:text-white"
            }`}
          >
            {maskHost(labelText())}
          </span>

          {hostLabel ? (
            <span className="shrink-0 rounded bg-[var(--border)]/60 px-1.5 py-0.2 font-mono text-[9px] text-[var(--text-faint)]">
              {maskHost(hostLabel)}
            </span>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {/* Diff stats pills */}
          {isEdit && (added > 0 || removed > 0) && (
            <span className="flex items-center gap-1 font-mono text-[10px]">
              {added > 0 && <span className="text-emerald-400 font-semibold">+{added}</span>}
              {removed > 0 && <span className="text-red-400 font-semibold">-{removed}</span>}
            </span>
          )}

          {hasExpandableContent && (
            <span className="font-mono text-[10px] text-gray-500 transition-transform group-hover:text-gray-300">
              {open ? "▾" : "›"}
            </span>
          )}
        </div>
      </div>

      {/* Expanded Details Sub-pane */}
      {open && hasExpandableContent && (
        <div className="border-t border-[var(--border)]/50 bg-[#080c12]/90 p-2.5">
          {/* Command execution detail */}
          {isCmd && (
            <div className="flex flex-col gap-2 font-mono text-[11px]">
              <div className="flex items-center justify-between gap-2 text-[10px] text-gray-500">
                <div className="flex min-w-0 flex-1 items-center gap-1.5">
                  <span className="shrink-0 font-bold text-emerald-400">$</span>
                  <ShellCommand code={maskHost(cmd)} className="min-w-0 flex-1" />
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    onClick={copyCommand}
                    className="rounded border border-[var(--border)] px-1.5 py-0.5 text-[9px] text-gray-400 hover:bg-[var(--border)] hover:text-white"
                  >
                    Copy cmd
                  </button>
                  {output && (
                    <button
                      type="button"
                      onClick={copyOutput}
                      className="rounded border border-[var(--border)] px-1.5 py-0.5 text-[9px] text-gray-400 hover:bg-[var(--border)] hover:text-white"
                    >
                      Copy output
                    </button>
                  )}
                  {hostLabel && (
                    <button
                      type="button"
                      onClick={openOnCanvas}
                      className="rounded border border-cyan-800/60 bg-cyan-950/40 px-1.5 py-0.5 text-[9px] text-cyan-300 hover:bg-cyan-900/60 hover:text-white"
                    >
                      Terminal ↗
                    </button>
                  )}
                </div>
              </div>

              {output ? (
                <div className="agent-activity-scroll max-h-[260px] overflow-y-auto rounded border border-[var(--border)]/60 bg-black/60 p-2">
                  <ConsoleOutput text={maskHost(output)} />
                </div>
              ) : running ? (
                <div className="text-[10px] text-cyan-400">Executing on remote host…</div>
              ) : (
                <div className="text-[10px] text-gray-600">(No output / exit code 0)</div>
              )}
            </div>
          )}

          {/* File Edit Diff View */}
          {isEdit && (
            <div className="flex flex-col gap-1">
              <div className="flex items-center justify-between text-[10px] text-gray-400">
                <span className="font-mono">{maskHost(fileName)}</span>
                <span className="font-mono text-[10px]">
                  {added > 0 && <span className="text-emerald-400">+{added} </span>}
                  {removed > 0 && <span className="text-red-400">-{removed}</span>}
                </span>
              </div>
              <DiffViewer hunks={hunks} fileName={fileName} />
            </div>
          )}

          {/* Read / Search / Other Output */}
          {!isCmd && !isEdit && (output || item.detail) && (
            <div className="agent-activity-scroll max-h-[240px] overflow-y-auto rounded border border-[var(--border)]/60 bg-black/50 p-2 font-mono text-[10px] text-gray-300 whitespace-pre-wrap">
              {maskHost(output || item.detail || "")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Dynamic thinking verbs. Text opacity only — no blur, canvas, or GPU filters. */
const THINKING_VERBS = [
  "Analyzing",
  "Architecting",
  "Brainstorming",
  "Checking",
  "Comparing",
  "Compiling",
  "Connecting",
  "Evaluating",
  "Examining",
  "Exploring",
  "Formulating",
  "Generating",
  "Inspecting",
  "Investigating",
  "Mapping",
  "Optimizing",
  "Parsing",
  "Planning",
  "Processing",
  "Reasoning",
  "Reflecting",
  "Scanning",
  "Searching",
  "Synthesizing",
  "Thinking",
  "Validating",
  "Verifying",
];

export function AgentThinking() {
  const [i, setI] = useState(() => Math.floor(Math.random() * THINKING_VERBS.length));
  const turnStartTime = useAgentStore((s) => s.turnStartTime);
  const [elapsedSecs, setElapsedSecs] = useState(0);

  useEffect(() => {
    const t = window.setInterval(() => {
      setI((n) => (n + 1) % THINKING_VERBS.length);
    }, 2400);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    const updateElapsed = () => {
      if (turnStartTime) {
        setElapsedSecs(Math.max(0, Math.floor((Date.now() - turnStartTime) / 1000)));
      } else {
        setElapsedSecs(0);
      }
    };
    updateElapsed();
    const interval = window.setInterval(updateElapsed, 500);
    return () => window.clearInterval(interval);
  }, [turnStartTime]);

  const formatElapsed = (sec: number) => {
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = sec % 60;
    if (h > 0) {
      return `${h}h ${m}m ${s.toString().padStart(2, "0")}s`;
    }
    return `${m}:${s.toString().padStart(2, "0")}`;
  };

  return (
    <div className="flex items-center gap-2 px-1 py-1 font-mono text-[11px]">
      <HashSpinner kind="think" />
      <span className="xc-think-verb text-[11px] text-[var(--text-faint)]">
        {THINKING_VERBS[i]}…
      </span>
      {turnStartTime && (
        <span className="flex items-center gap-1 rounded bg-cyan-950/60 px-1.5 py-0.5 text-[10px] text-cyan-300 font-mono border border-cyan-500/20">
          <span className="relative flex h-1.5 w-1.5">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-cyan-400 opacity-75"></span>
            <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-cyan-500"></span>
          </span>
          {formatElapsed(elapsedSecs)}
        </span>
      )}
    </div>
  );
}

/**
 * Unified, professional Activity Feed matching Claude Code & Cursor design.
 * Automatically groups and collapses tool activities into an elegant summary row,
 * with expand-to-inspect diff viewer, terminal outputs, and tool history.
 */
export const AgentActivityFeed = memo(function AgentActivityFeed({
  items,
  live = false,
}: {
  items: AgentActivityItem[];
  live?: boolean;
}) {
  const visible = useMemo(() => visibleActivityItems(items), [items]);
  const [feedExpanded, setFeedExpanded] = useState(false);
  const [expandedItemId, setExpandedItemId] = useState<string | null>(null);

  const runningItems = useMemo(
    () => visible.filter((i) => i.state === "running"),
    [visible],
  );
  const failedItems = useMemo(
    () => visible.filter((i) => i.state === "error"),
    [visible],
  );

  const diffStats = useMemo(() => totalDiffStats(visible), [visible]);
  const summaryText = useMemo(() => activitySummary(visible), [visible]);

  if (visible.length === 0 && !live) return null;

  const isLiveRunning = live && runningItems.length > 0;
  const isFailed = failedItems.length > 0;

  const copyAllActivity = () => {
    const lines = visible.map((item) => {
      if (isCommandItem(item)) {
        const cmd = redactExportText(item.detail || item.label);
        const out = item.output ? redactExportText(item.output) : "";
        return `$ ${cmd}${out ? `\n${out}` : ""}`;
      }
      if (isFileEditItem(item)) {
        return `edit ${redactExportText(item.path || item.label)} +${item.linesAdded ?? 0}/-${item.linesRemoved ?? 0}`;
      }
      return redactExportText(item.label + (item.detail ? ` — ${item.detail}` : ""));
    });
    void navigator.clipboard.writeText(lines.join("\n"));
  };

  return (
    <div className="my-1.5 flex w-full flex-col overflow-hidden rounded-lg border border-[var(--border)] bg-[#0a0e14]/90 shadow-md backdrop-blur-md transition-all">
      {/* Top Header / Summary Bar */}
      <div
        onClick={() => setFeedExpanded((v) => !v)}
        className="flex cursor-pointer select-none items-center justify-between gap-2 px-3 py-2 text-[11px] font-mono hover:bg-[#121820]"
      >
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {/* Status Indicator */}
          {isLiveRunning ? (
            <span className="relative flex h-2.5 w-2.5">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-cyan-400 opacity-75"></span>
              <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-cyan-500"></span>
            </span>
          ) : isFailed ? (
            <span className="h-2 w-2 rounded-full bg-red-500" />
          ) : (
            <span className="h-2 w-2 rounded-full bg-emerald-500" />
          )}

          {/* Primary Summary Text */}
          <span className="min-w-0 flex-1 truncate font-medium text-gray-200">
            {isLiveRunning
              ? runningItems.length > 1
                ? `Running ${runningItems.length} tools in parallel…`
                : `${liveGerund(runningItems[0])}…`
              : summaryText}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {/* Total Diff Badge (if files edited) */}
          {(diffStats.added > 0 || diffStats.removed > 0) && (
            <span className="flex items-center gap-1 rounded bg-[#161f2c] px-2 py-0.5 font-mono text-[10px]">
              {diffStats.added > 0 && (
                <span className="font-semibold text-emerald-400">+{diffStats.added}</span>
              )}
              {diffStats.removed > 0 && (
                <span className="font-semibold text-red-400">-{diffStats.removed}</span>
              )}
            </span>
          )}

          <span className="text-[10px] text-gray-500 hover:text-gray-300">
            {feedExpanded ? "hide" : `${visible.length} action${visible.length === 1 ? "" : "s"}`}
          </span>
          <span className="font-mono text-[10px] text-gray-500">
            {feedExpanded ? "▾" : "›"}
          </span>
        </div>
      </div>

      {/* Expanded List of Actions */}
      {feedExpanded && (
        <div className="flex flex-col border-t border-[var(--border)]/70 bg-[#070b10]">
          <div className="divide-y divide-[var(--border)]/40">
            {visible.map((item) => (
              <ActivityItemRow
                key={`${item.id}-${item.kind}`}
                item={item}
                open={expandedItemId === item.id}
                onToggle={() =>
                  setExpandedItemId((cur) => (cur === item.id ? null : item.id))
                }
              />
            ))}
          </div>

          {/* Footer toolbar inside expanded view */}
          <div className="flex items-center justify-between border-t border-[var(--border)]/60 bg-[#0a0e14] px-3 py-1.5 text-[10px] text-gray-500">
            <span>
              {visible.length} total step{visible.length === 1 ? "" : "s"}
            </span>
            <button
              type="button"
              onClick={copyAllActivity}
              className="rounded px-2 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-white"
            >
              Copy all logs
            </button>
          </div>
        </div>
      )}
    </div>
  );
});
