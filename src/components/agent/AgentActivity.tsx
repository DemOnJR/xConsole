import { useMemo } from "react";
import { SettingsIcon, TerminalIcon } from "../icons";
import type { AgentActivityItem } from "../../stores/agentStore";
import { CodeHighlight, ConsoleOutput, langFromPath, ShellCommand } from "./SyntaxHighlight";
import { useVpsStore } from "../../stores/vpsStore";
import { useCanvasStore } from "../../stores/canvasStore";
import { useUiStore } from "../../stores/uiStore";

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
    // Parallel-batch status is the one status line users care about.
    if (item.kind === "status") {
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

function MetaLine({
  text,
  dimmed,
  running,
}: {
  text: string;
  dimmed?: boolean;
  running?: boolean;
}) {
  return (
    <div
      className={`flex items-center gap-1.5 text-[11px] leading-[1.35] ${
        dimmed ? "text-gray-600" : "text-gray-500"
      }`}
    >
      {running ? (
        <span
          className="inline-block h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-cyan-500/80"
          aria-hidden
        />
      ) : null}
      <span className="min-w-0 truncate">{text}</span>
    </div>
  );
}

/** Banner shown while several read-only tools run concurrently. */
function ParallelBanner({
  count,
  label,
  done,
}: {
  count: number;
  label?: string;
  done?: boolean;
}) {
  return (
    <div
      className={`flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-[11px] ${
        done
          ? "border-[var(--border)] bg-[var(--surface)]/40 text-gray-500"
          : "border-cyan-800/50 bg-cyan-950/35 text-cyan-200/90"
      }`}
    >
      {done ? (
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--success)]" />
      ) : (
        <span
          className="inline-block h-2 w-2 shrink-0 animate-spin rounded-full border border-cyan-400/70 border-t-transparent"
          aria-hidden
        />
      )}
      <span className="min-w-0 flex-1 truncate font-medium">
        {label?.trim() ||
          (done
            ? `Finished ${count} tools in parallel`
            : `Running ${count} tools in parallel`)}
      </span>
      {!done && count > 0 ? (
        <span className="shrink-0 rounded bg-cyan-900/50 px-1.5 py-px font-mono text-[10px] text-cyan-300/90">
          ×{count}
        </span>
      ) : null}
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
    <div className="overflow-hidden rounded-lg border border-[var(--border)] bg-[#0d1118]">
      <div className="flex items-center gap-2 border-b border-[var(--border)]/80 px-2.5 py-1.5">
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

/** Extract host label from "Run on <name>" activity titles. */
function hostFromCommandLabel(label: string): string | null {
  const m = /^Run on (.+)$/i.exec(label.trim());
  return m?.[1]?.trim() || null;
}

function CommandCard({ item }: { item: AgentActivityItem }) {
  const running = item.state === "running";
  const failed = item.state === "error";
  const cmd = commandBody(item);
  const output = item.output?.trim();
  const hostLabel = hostFromCommandLabel(item.label);
  const vpsList = useVpsStore((s) => s.vpsList);
  const addVps = useCanvasStore((s) => s.addVps);
  const focus = useCanvasStore((s) => s.focus);
  const nodes = useCanvasStore((s) => s.nodes);

  const openOnCanvas = () => {
    if (!hostLabel) return;
    const vps = vpsList.find(
      (v) => v.name === hostLabel || v.host === hostLabel || v.id === hostLabel,
    );
    if (!vps) return;
    // Reuse an existing terminal for this host when possible.
    const existing = nodes.find(
      (n) => n.type === "terminal" && String(n.data.vpsId) === vps.id,
    );
    if (existing) {
      focus(existing.id);
    } else {
      const id = addVps(vps);
      focus(id);
    }
    // Keep agent visible; user can still see the canvas terminal.
    useUiStore.getState().setAgentExpanded(false);
  };

  return (
    <div
      className={`overflow-hidden rounded-[var(--radius-md)] border bg-[var(--bg)] ${
        failed
          ? "border-[color-mix(in_srgb,var(--danger)_45%,var(--border))]"
          : running
            ? "border-[color-mix(in_srgb,var(--accent)_35%,var(--border))]"
            : "border-[var(--border)]"
      }`}
    >
      <div className="flex items-center gap-2 border-b border-[var(--border)]/80 px-2.5 py-1.5">
        <TerminalIcon size={12} className="shrink-0 text-[var(--text-faint)]" />
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--text-dim)]">
          {hostLabel ? (
            <>
              <span className="text-[var(--text-faint)]">{hostLabel}</span>
              <span className="mx-1 text-[var(--border-strong)]">·</span>
              {commandTitle(item)}
            </>
          ) : (
            commandTitle(item)
          )}
        </span>
        {hostLabel ? (
          <button
            type="button"
            className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-[var(--text-faint)] transition hover:bg-[var(--surface-hover)] hover:text-[var(--accent)]"
            data-tooltip="Open this host on the canvas"
            onClick={openOnCanvas}
          >
            Canvas
          </button>
        ) : null}
        {running ? (
          <span
            className="inline-block h-2 w-2 animate-spin rounded-full border border-[var(--text-faint)] border-t-transparent"
            aria-label="Running"
          />
        ) : failed ? (
          <span className="h-1.5 w-1.5 rounded-full bg-[var(--danger)]" title="Failed" />
        ) : (
          <span className="h-1.5 w-1.5 rounded-full bg-[var(--success)]" title="Done" />
        )}
      </div>
      <div className="agent-activity-scroll max-h-[280px] overflow-y-auto px-2.5 py-2 font-[family-name:var(--font-mono)]">
        <div className="flex gap-1.5">
          <span className="shrink-0 select-none font-mono text-[10px] text-[var(--success)]">
            $
          </span>
          <ShellCommand code={cmd} className="min-w-0 flex-1" />
        </div>
        {output && !running ? (
          <div className="mt-2 border-t border-[var(--border)]/60 pt-2">
            <ConsoleOutput text={output} />
          </div>
        ) : null}
        {running && !output ? (
          <div className="mt-2 text-[10px] text-[var(--text-faint)]">Running on host…</div>
        ) : null}
      </div>
    </div>
  );
}

function ActivityBlock({ item }: { item: AgentActivityItem }) {
  if (item.kind === "status" && (item.id === "parallel-batch" || /parallel/i.test(item.label))) {
    // Banner is rendered once by the feed when grouping; skip duplicate rows.
    return null;
  }
  if (item.kind === "file_edit") {
    return <FileEditCard item={item} />;
  }
  if (isCommandItem(item)) {
    return <CommandCard item={item} />;
  }
  if (isMetaItem(item)) {
    return (
      <MetaLine
        text={metaLine(item)}
        running={item.state === "running"}
        dimmed={item.state === "done"}
      />
    );
  }
  return (
    <MetaLine
      text={metaLine(item)}
      dimmed={item.state === "running"}
      running={item.state === "running"}
    />
  );
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

  const parallelMeta = useMemo(() => {
    const banner = visible.find(
      (i) => i.kind === "status" && (i.id === "parallel-batch" || /parallel/i.test(i.label)),
    );
    const running = visible.filter(
      (i) => i.state === "running" && i.kind !== "status" && i.id !== "parallel-batch",
    );
    // Show banner when backend announced parallel, or live with 2+ concurrent tools.
    const show = Boolean(banner) || (live && running.length >= 2);
    const done = banner ? banner.state === "done" : false;
    return {
      show,
      done,
      count: running.length || (banner ? 0 : 0),
      label: banner?.label,
      // Prefer live running count; fall back to parsing "Running N …" from status.
      displayCount:
        running.length ||
        (() => {
          const m = banner?.label?.match(/(\d+)/);
          return m ? Number(m[1]) : 0;
        })(),
    };
  }, [visible, live]);

  const blocks = useMemo(
    () =>
      visible.filter(
        (item) =>
          !(
            item.kind === "status" &&
            (item.id === "parallel-batch" || /parallel/i.test(item.label))
          ),
      ),
    [visible],
  );

  // Collapse long finished meta lines so command/edit cards stay scannable.
  const META_KEEP_TAIL = 4;
  const collapsedBlocks = useMemo(() => {
    if (live) return blocks;
    const isMetaDone = (item: AgentActivityItem) =>
      item.state !== "running" &&
      item.kind !== "file_edit" &&
      item.kind !== "command" &&
      !isCommandItem(item);
    const metaDoneIdxs = blocks
      .map((b, i) => (isMetaDone(b) ? i : -1))
      .filter((i) => i >= 0);
    if (metaDoneIdxs.length <= META_KEEP_TAIL + 2) return blocks;
    const drop = new Set(metaDoneIdxs.slice(0, metaDoneIdxs.length - META_KEEP_TAIL));
    const kept: AgentActivityItem[] = [];
    let collapsed = 0;
    let inserted = false;
    for (let i = 0; i < blocks.length; i++) {
      if (drop.has(i)) {
        collapsed += 1;
        if (!inserted) {
          kept.push({
            id: "collapsed-meta",
            kind: "status",
            label: `${collapsed} earlier steps`,
            state: "done",
          });
          inserted = true;
        } else {
          const last = kept[kept.length - 1];
          if (last.id === "collapsed-meta") {
            kept[kept.length - 1] = {
              ...last,
              label: `${collapsed} earlier steps`,
            };
          }
        }
        continue;
      }
      kept.push(blocks[i]);
    }
    return kept;
  }, [blocks, live]);

  if (visible.length === 0 && !live) return null;

  const copyActivity = () => {
    const lines = collapsedBlocks.map((item) => {
      if (item.kind === "command") {
        return `$ ${item.detail || item.label}${item.output ? `\n${item.output}` : ""}`;
      }
      if (item.kind === "file_edit") {
        return `edit ${item.path || item.label} +${item.linesAdded ?? 0}/-${item.linesRemoved ?? 0}`;
      }
      return item.label + (item.detail ? ` — ${item.detail}` : "");
    });
    void navigator.clipboard.writeText(lines.join("\n"));
  };

  return (
    <div className="flex w-full flex-col gap-2">
      {parallelMeta.show ? (
        <ParallelBanner
          count={parallelMeta.displayCount}
          label={parallelMeta.label}
          done={parallelMeta.done && !live}
        />
      ) : null}
      {!live && collapsedBlocks.length > 2 ? (
        <button
          type="button"
          onClick={copyActivity}
          className="self-start text-[10px] text-gray-600 hover:text-gray-400"
          data-tooltip="Copy activity log as text"
        >
          Copy activity
        </button>
      ) : null}
      {collapsedBlocks.map((item) =>
        item.id === "collapsed-meta" ? (
          <MetaLine key="collapsed-meta" text={item.label} dimmed />
        ) : (
          <ActivityBlock key={`${item.id}-${item.kind}`} item={item} />
        ),
      )}
      {live && blocks.length > 0 && !parallelMeta.show && (
        <MetaLine text="Planning next moves" dimmed />
      )}
      {live && parallelMeta.show && !parallelMeta.done && parallelMeta.displayCount > 0 && (
        <MetaLine text="Tools running together…" dimmed />
      )}
    </div>
  );
}
