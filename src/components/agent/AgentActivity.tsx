import { useEffect, useMemo, useState } from "react";
import { SettingsIcon } from "../icons";
import type { AgentActivityItem } from "../../stores/agentStore";
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

function isTodoItem(item: AgentActivityItem): boolean {
  return (
    item.tool === "todo_write" ||
    /^update checklist$/i.test(item.label.trim()) ||
    /^todo write$/i.test(item.label.trim())
  );
}

function TodoCard({ item }: { item: AgentActivityItem }) {
  const lines = (item.output || item.detail || "").split("\n").filter(Boolean);
  return (
    <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--surface)]/50 px-2.5 py-1.5">
      <div className="mb-1 text-[10px] uppercase tracking-wide text-[var(--text-faint)]">
        Checklist
      </div>
      <ul className="flex flex-col gap-0.5 font-mono text-[11px] text-[var(--text-dim)]">
        {lines.map((line, i) => {
          const done = line.startsWith("[x]");
          const active = line.startsWith("[>]");
          return (
            <li
              key={i}
              className={
                done
                  ? "text-[var(--text-faint)] line-through"
                  : active
                    ? "text-[var(--accent)]"
                    : ""
              }
            >
              {line}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function isMetaItem(item: AgentActivityItem): boolean {
  if (item.kind === "file_edit" || isCommandItem(item) || isTodoItem(item)) return false;
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
  const cmd = redactExportText(
    item.detail?.trim() ||
      item.label.replace(/^(SSH|Shell)\s*›\s*/i, "").trim() ||
      item.label.replace(/^Run on [^:]+:\s*/i, "").trim(),
  );
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
  item,
}: {
  text: string;
  dimmed?: boolean;
  running?: boolean;
  item?: AgentActivityItem;
}) {
  const maskHost = useMaskHost();
  return (
    <div
      className={`flex items-center gap-1.5 text-[11px] leading-[1.35] ${
        dimmed ? "text-gray-600" : "text-gray-500"
      }`}
    >
      {running ? <HashSpinner item={item} /> : null}
      <span className="min-w-0 truncate">{maskHost(text)}</span>
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

function CommandCard({
  item,
  defaultCollapsed = true,
  open,
  onOpenChange,
}: {
  item: AgentActivityItem;
  defaultCollapsed?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const maskHost = useMaskHost();
  const running = item.state === "running";
  const failed = item.state === "error";
  const [internal, setInternal] = useState(!defaultCollapsed);
  const expanded = open ?? internal;
  const setExpanded = (next: boolean | ((v: boolean) => boolean)) => {
    const computed = typeof next === "function" ? next(expanded) : next;
    setInternal(computed);
    onOpenChange?.(computed);
  };
  const cmd = redactExportText(commandBody(item));
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
  };

  return (
    <div
      className={`overflow-hidden rounded-md border bg-[var(--surface)]/30 ${
        failed
          ? "border-[color-mix(in_srgb,var(--danger)_45%,var(--border))]"
          : running
            ? "border-[color-mix(in_srgb,var(--accent)_35%,var(--border))]"
            : "border-[var(--border)]/60"
      }`}
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-2.5 py-1 text-left transition hover:bg-[var(--surface-hover)]"
        onClick={() => setExpanded((v) => !v)}
        data-tooltip={expanded ? "Collapse" : "Expand command details"}
      >
        <span className="select-none font-mono text-[9px] text-[var(--text-faint)]">
          {expanded ? "▼" : "▶"}
        </span>
        <span className="shrink-0 font-mono text-[10px] text-[var(--success)]">$</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--text-dim)]">
          {maskHost(cmd || commandTitle(item))}
        </span>
        {hostLabel ? (
          <span className="shrink-0 rounded bg-[var(--border)]/60 px-1 py-0.5 font-mono text-[9px] text-[var(--text-faint)]">
            {maskHost(hostLabel)}
          </span>
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
      </button>
      {expanded && (
        <div className="agent-activity-scroll max-h-[280px] overflow-y-auto border-t border-[var(--border)]/60 bg-[var(--bg)] px-2.5 py-2 font-[family-name:var(--font-mono)]">
          <div className="flex items-center justify-between gap-2 pb-1 text-[10px] text-[var(--text-faint)]">
            <span className="truncate">{maskHost(commandTitle(item))}</span>
            {hostLabel ? (
              <button
                type="button"
                className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-[var(--text-faint)] transition hover:bg-[var(--surface-hover)] hover:text-[var(--accent)]"
                data-tooltip={`Open ${maskHost(hostLabel)} on the canvas`}
                onClick={(e) => {
                  e.stopPropagation();
                  openOnCanvas();
                }}
              >
                Open on Canvas
              </button>
            ) : null}
          </div>
          <div className="flex gap-1.5">
            <span className="shrink-0 select-none font-mono text-[10px] text-[var(--success)]">
              $
            </span>
            <ShellCommand code={maskHost(cmd)} className="min-w-0 flex-1" />
          </div>
          {output && !running ? (
            <div className="mt-2 border-t border-[var(--border)]/60 pt-2">
              <ConsoleOutput text={maskHost(redactExportText(output))} />
            </div>
          ) : null}
          {running && !output ? (
            <div className="mt-2 text-[10px] text-[var(--text-faint)]">Running on host…</div>
          ) : null}
        </div>
      )}
    </div>
  );
}

function ActivityBlock({ item, defaultCollapsed = true }: { item: AgentActivityItem; defaultCollapsed?: boolean }) {
  if (item.kind === "status" && (item.id === "parallel-batch" || /parallel/i.test(item.label))) {
    // Banner is rendered once by the feed when grouping; skip duplicate rows.
    return null;
  }
  if (item.kind === "file_edit") {
    return <FileEditCard item={item} />;
  }
  if (isTodoItem(item)) {
    return <TodoCard item={item} />;
  }
  if (isCommandItem(item)) {
    return <CommandCard item={item} defaultCollapsed={defaultCollapsed} />;
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

/** Dynamic thinking verbs. Text opacity only — no blur, canvas, or GPU filters. */
const THINKING_VERBS = [
  "Accomplishing",
  "Actioning",
  "Actualizing",
  "Analyzing",
  "Architecting",
  "Baking",
  "Beaming",
  "Beboppin'",
  "Befuddling",
  "Billowing",
  "Blanching",
  "Bloviating",
  "Boogieing",
  "Boondoggling",
  "Booping",
  "Bootstrapping",
  "Brainstorming",
  "Breathing",
  "Brewing",
  "Bunning",
  "Burrowing",
  "Calculating",
  "Canoodling",
  "Caramelizing",
  "Cascading",
  "Catapulting",
  "Cerebrating",
  "Channeling",
  "Channelling",
  "Choreographing",
  "Churning",
  "Classifying",
  "Coalescing",
  "Cogitating",
  "Combobulating",
  "Comparing",
  "Composing",
  "Computing",
  "Conceptualizing",
  "Concluding",
  "Concocting",
  "Considering",
  "Contemplating",
  "Contrasting",
  "Cooking",
  "Crafting",
  "Creating",
  "Crunching",
  "Crystallizing",
  "Cultivating",
  "Deciphering",
  "Deconstructing",
  "Deliberating",
  "Determining",
  "Dilly-dallying",
  "Discombobulating",
  "Doing",
  "Doodling",
  "Drizzling",
  "Ebbing",
  "Effecting",
  "Elucidating",
  "Embellishing",
  "Enchanting",
  "Envisioning",
  "Evaluating",
  "Evaporating",
  "Fermenting",
  "Fiddle-faddling",
  "Finagling",
  "Flambéing",
  "Flibbertigibbeting",
  "Flowing",
  "Flummoxing",
  "Fluttering",
  "Forging",
  "Forming",
  "Frolicking",
  "Frosting",
  "Gallivanting",
  "Galloping",
  "Garnishing",
  "Generating",
  "Gesticulating",
  "Germinating",
  "Gitifying",
  "Grooving",
  "Gusting",
  "Harmonizing",
  "Hashing",
  "Hatching",
  "Herding",
  "Honking",
  "Hullaballooing",
  "Hyperspacing",
  "Hypothesizing",
  "Ideating",
  "Imagining",
  "Improvising",
  "Incubating",
  "Inferring",
  "Infusing",
  "Innovating",
  "Ionizing",
  "Jitterbugging",
  "Julienning",
  "Kneading",
  "Leavening",
  "Levitating",
  "Lollygagging",
  "Manifesting",
  "Marinating",
  "Meandering",
  "Metamorphosing",
  "Misting",
  "Moonwalking",
  "Moseying",
  "Mulling",
  "Musing",
  "Mustering",
  "Nebulizing",
  "Nesting",
  "Newspapering",
  "Noodling",
  "Noticing",
  "Nucleating",
  "Orbiting",
  "Orchestrating",
  "Osmosing",
  "Perambulating",
  "Perceiving",
  "Percolating",
  "Perusing",
  "Philosophising",
  "Photosynthesizing",
  "Pollinating",
  "Pondering",
  "Pontificating",
  "Pouncing",
  "Precipitating",
  "Prestidigitating",
  "Processing",
  "Proofing",
  "Propagating",
  "Puttering",
  "Puzzling",
  "Quantumizing",
  "Razzle-dazzling",
  "Razzmatazzing",
  "Recalling",
  "Recognizing",
  "Recombobulating",
  "Reconsidering",
  "Reflecting",
  "Remembering",
  "Reticulating",
  "Roosting",
  "Ruminating",
  "Sautéing",
  "Scampering",
  "Schlepping",
  "Scurrying",
  "Seasoning",
  "Shenaniganing",
  "Shimmying",
  "Simmering",
  "Skedaddling",
  "Sketching",
  "Slithering",
  "Smooshing",
  "Sock-hopping",
  "Spelunking",
  "Spinning",
  "Sprouting",
  "Stewing",
  "Sublimating",
  "Swirling",
  "Swooping",
  "Symbioting",
  "Synthesizing",
  "Tempering",
  "Thinking",
  "Thundering",
  "Tinkering",
  "Tomfoolering",
  "Topsy-turvying",
  "Transfiguring",
  "Transmuting",
  "Twisting",
  "Undulating",
  "Unfurling",
  "Unravelling",
  "Vibing",
  "Waddling",
  "Wandering",
  "Warping",
  "Whatchamacalliting",
  "Whirlpooling",
  "Whirring",
  "Whisking",
  "Wibbling",
  "Working",
  "Wrangling",
  "Zesting",
  "Zigzagging",
];

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
    else if (item.kind === "file_edit" || item.tool === "write_file") writes += 1;
    else if (item.tool === "read_file" || /^read /i.test(item.label)) reads += 1;
  }
  const parts: string[] = [];
  if (commands) parts.push(`executed ${commands} command${commands === 1 ? "" : "s"}`);
  if (reads) parts.push(`read ${reads} file${reads === 1 ? "" : "s"}`);
  if (writes) parts.push(`wrote ${writes} file${writes === 1 ? "" : "s"}`);
  if (parts.length === 0) return `${visible.length} step${visible.length === 1 ? "" : "s"}`;
  return parts.join(" · ");
}

export function AgentThinking() {
  const [i, setI] = useState(() => Math.floor(Math.random() * THINKING_VERBS.length));
  useEffect(() => {
    const t = window.setInterval(() => {
      setI((n) => (n + 1) % THINKING_VERBS.length);
    }, 2400);
    return () => window.clearInterval(t);
  }, []);
  return (
    <div className="flex items-center gap-2 px-1 py-1">
      <HashSpinner kind="think" />
      <span className="xc-think-verb text-[11px] text-[var(--text-faint)]">
        {THINKING_VERBS[i]}…
      </span>
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
  const [commandsOpen, setCommandsOpen] = useState(false);
  const [openCommandId, setOpenCommandId] = useState<string | null>(null);

  const parallelMeta = useMemo(() => {
    const banner = visible.find(
      (i) => i.kind === "status" && (i.id === "parallel-batch" || /parallel/i.test(i.label)),
    );
    const running = visible.filter(
      (i) => i.state === "running" && i.kind !== "status" && i.id !== "parallel-batch",
    );
    const show = Boolean(banner) || (live && running.length >= 2);
    const done = banner ? banner.state === "done" : false;
    return {
      show,
      done,
      count: running.length,
      label: banner?.label,
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

  // Collapse completed commands into the summary accordion.
  const doneCommands = useMemo(
    () =>
      blocks.filter(
        (item) => isCommandItem(item) && item.state !== "running" && item.state !== "error",
      ),
    [blocks],
  );
  const rest = useMemo(
    () =>
      blocks.filter(
        (item) => !(isCommandItem(item) && item.state !== "running" && item.state !== "error"),
      ),
    [blocks],
  );

  if (visible.length === 0 && !live) return null;

  const copyActivity = () => {
    const lines = blocks.map((item) => {
      if (isCommandItem(item)) {
        const cmd = redactExportText(item.detail || item.label);
        const out = item.output ? redactExportText(item.output) : "";
        return `$ ${cmd}${out ? `\n${out}` : ""}`;
      }
      if (item.kind === "file_edit") {
        return `edit ${redactExportText(item.path || item.label)} +${item.linesAdded ?? 0}/-${item.linesRemoved ?? 0}`;
      }
      return redactExportText(item.label + (item.detail ? ` — ${item.detail}` : ""));
    });
    void navigator.clipboard.writeText(lines.join("\n"));
  };

  const n = doneCommands.length;
  const summary = activitySummary(blocks);
  const running = rest.filter((item) => item.state === "running");

  return (
    <div className="flex w-full flex-col gap-2">
      {parallelMeta.show ? (
        <ParallelBanner
          count={parallelMeta.displayCount}
          label={parallelMeta.label}
          done={parallelMeta.done && !live}
        />
      ) : null}
      {!live && blocks.length > 2 ? (
        <button
          type="button"
          onClick={copyActivity}
          className="self-start text-[10px] text-gray-600 hover:text-gray-400"
          data-tooltip="Copy activity log as text"
        >
          Copy activity
        </button>
      ) : null}
      {live && running.length > 0 ? (
        <div className="flex flex-col gap-0.5">
          {running.slice(0, 3).map((item) => (
            <MetaLine key={`live-${item.id}`} text={`${liveGerund(item)}…`} running item={item} />
          ))}
        </div>
      ) : null}
      {n > 0 && (
        <div className="overflow-hidden rounded-md border border-[var(--border)] bg-[var(--surface)]/40">
          <button
            type="button"
            onClick={() => {
              setCommandsOpen((v) => !v);
              if (commandsOpen) setOpenCommandId(null);
            }}
            className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-left text-[11px] text-gray-500 hover:bg-[var(--border)] hover:text-gray-300"
            data-tooltip={commandsOpen ? "Hide commands" : "Show commands"}
            aria-expanded={commandsOpen}
          >
            <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--success)]" />
            <span className="min-w-0 flex-1 truncate">{summary}</span>
            <span className="text-[10px] text-gray-600">{commandsOpen ? "hide" : "show"}</span>
          </button>
          {commandsOpen && (
            <div className="flex flex-col gap-1.5 border-t border-[var(--border)]/70 p-1.5">
              {doneCommands.map((item) => (
                <CommandCard
                  key={`${item.id}-${item.kind}`}
                  item={item}
                  defaultCollapsed
                  open={openCommandId === item.id}
                  onOpenChange={(next) => setOpenCommandId(next ? item.id : null)}
                />
              ))}
            </div>
          )}
        </div>
      )}
      {rest.map((item) => (
        <ActivityBlock key={`${item.id}-${item.kind}`} item={item} defaultCollapsed={!live} />
      ))}
      {live && parallelMeta.show && !parallelMeta.done && parallelMeta.displayCount > 0 && (
        <MetaLine text="Tools running together…" dimmed />
      )}
    </div>
  );
}
