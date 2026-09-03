import type { StreamEvent } from "../lib/tauri";
import type { AgentActivityItem, AgentChatMessage } from "./agentStore";

/** One chronological slice of an assistant turn: prose or a tool burst. */
export type TurnSegment =
  | { type: "text"; content: string }
  | { type: "activity"; items: AgentActivityItem[] };

export function appendTextDelta(segments: TurnSegment[], delta: string): TurnSegment[] {
  if (!delta) return segments;
  const last = segments[segments.length - 1];
  if (last?.type === "text") {
    return [...segments.slice(0, -1), { type: "text", content: last.content + delta }];
  }
  return [...segments, { type: "text", content: delta }];
}

export function applyActivityEvent(segments: TurnSegment[], ev: StreamEvent): TurnSegment[] {
  const ids = eventTargetIds(ev);
  if (ids.length > 0) {
    const si = segments.findIndex(
      (s) => s.type === "activity" && s.items.some((item) => itemMatchesIds(item, ids)),
    );
    if (si >= 0) {
      const next = segments.slice();
      const seg = next[si];
      if (seg.type === "activity") {
        next[si] = { type: "activity", items: applyStreamEvent(seg.items, ev) };
      }
      return next;
    }
  }
  const last = segments[segments.length - 1];
  if (last?.type === "activity") {
    return [
      ...segments.slice(0, -1),
      { type: "activity", items: applyStreamEvent(last.items, ev) },
    ];
  }
  const items = applyStreamEvent([], ev);
  if (items.length === 0) return segments;
  return [...segments, { type: "activity", items }];
}

export function flattenActivity(segments: TurnSegment[]): AgentActivityItem[] {
  return segments.flatMap((s) => (s.type === "activity" ? s.items : []));
}

export function textFromSegments(segments: TurnSegment[]): string {
  return segments
    .filter((s): s is { type: "text"; content: string } => s.type === "text")
    .map((s) => s.content)
    .join("\n\n");
}

/** History without `segments` keeps the old layout (text, then tools). */
export function segmentsFromMessage(message: AgentChatMessage): TurnSegment[] {
  if (message.segments && message.segments.length > 0) {
    return message.segments;
  }
  const segs: TurnSegment[] = [];
  if (message.content.trim()) {
    segs.push({ type: "text", content: message.content });
  }
  if ((message.activity?.length ?? 0) > 0) {
    segs.push({ type: "activity", items: message.activity ?? [] });
  }
  return segs;
}

function eventTargetIds(ev: StreamEvent): string[] {
  if (ev.kind === "ToolCall") return [ev.data.id];
  if (ev.kind === "ToolResult") return [ev.data.id];
  if (ev.kind === "Status" && /parallel/i.test(ev.data)) return ["parallel-batch"];
  if (ev.kind !== "Activity") return [];
  const d = ev.data;
  switch (d.type) {
    case "ToolStart":
    case "FileEdit":
    case "ToolEnd":
    case "Command":
      return [d.data.id];
    case "SkillRead":
      return [`${d.data.id}-skill-read`, d.data.id];
    case "SkillSaved":
      return [`${d.data.id}-skill-save`, d.data.id];
    default:
      return [];
  }
}

function itemMatchesIds(item: AgentActivityItem, ids: string[]): boolean {
  return ids.some((id) => item.id === id || item.id.startsWith(`${id}-`));
}

/** Apply a stream event to a flat activity list (one tool-burst). */
export function applyStreamEvent(
  activity: AgentActivityItem[],
  ev: StreamEvent,
): AgentActivityItem[] {
  switch (ev.kind) {
    case "Status": {
      if (/^cache(?: miss)?[:\s]/i.test(ev.data)) {
        return activity;
      }
      if (/parallel/i.test(ev.data)) {
        return [
          ...activity.filter((a) => a.id !== "parallel-batch"),
          {
            id: "parallel-batch",
            kind: "status" as const,
            label: ev.data,
            state: "running" as const,
          },
        ];
      }
      return activity;
    }
    case "ToolCall":
      if (activity.some((a) => a.id === ev.data.id)) return activity;
      if (/mcp/i.test(ev.data.name)) return activity;
      return [
        ...activity,
        {
          id: ev.data.id,
          kind: "tool",
          label: ev.data.name.replace(/_/g, " "),
          tool: ev.data.name,
          // The whole call, not just its name. "read file" tells a reader nothing about
          // which file on which server; the backend has always sent the arguments and
          // this is where they used to be dropped.
          arguments: ev.data.arguments,
          startedAt: now(),
          state: "running",
        },
      ];
    case "ToolResult": {
      if (ev.data.id.startsWith("snapshot-")) return activity;
      const idx = activity.findIndex((a) => a.id === ev.data.id);
      let next = activity;
      if (idx >= 0) {
        next = [...activity];
        next[idx] = { ...next[idx], ...outcomeOf(next[idx], ev.data.output) };
      }
      const stillRunning = next.some(
        (a) => a.state === "running" && a.id !== "parallel-batch" && a.kind !== "status",
      );
      if (!stillRunning) {
        next = next.map((a) =>
          a.id === "parallel-batch" && a.state === "running"
            ? ({
                ...a,
                state: "done" as const,
                label: a.label.replace(/…$/, " — done"),
              } satisfies AgentActivityItem)
            : a,
        );
      }
      return next;
    }
    case "Activity": {
      const d = ev.data;
      switch (d.type) {
        case "ToolStart":
          return [
            ...activity.filter((a) => !(a.id === d.data.id && a.kind === "tool")),
            {
              id: d.data.id,
              kind: "tool",
              tool: d.data.tool,
              label: d.data.label,
              detail: d.data.detail,
              state: "running",
            },
          ];
        case "FileEdit":
          return [
            ...activity.filter((a) => a.id !== d.data.id),
            {
              id: d.data.id,
              kind: "file_edit",
              label: d.data.path,
              path: d.data.path,
              linesAdded: d.data.lines_added,
              linesRemoved: d.data.lines_removed,
              hunks: d.data.hunks,
              state: "done",
            },
          ];
        case "ToolEnd": {
          const endState: "done" | "error" = d.data.ok ? "done" : "error";
          const afterEnd: AgentActivityItem[] = activity.map((a) => {
            if (a.id !== d.data.id && !a.id.startsWith(`${d.data.id}-`)) return a;
            if (a.kind === "file_edit") {
              return { ...a, state: endState, ...stamp(a) };
            }
            if (
              a.kind === "tool" &&
              a.label.startsWith("Write file ·") &&
              a.detail &&
              !activity.some((x) => x.id === a.id && x.kind === "file_edit")
            ) {
              const fullPath = a.label.slice("Write file ·".length).trim();
              const fileName = fullPath.split(/[/\\]/).pop() || fullPath;
              const hunks = a.detail.split("\n").slice(0, 28).map((text) => ({
                kind: "add" as const,
                text,
              }));
              return {
                id: a.id,
                kind: "file_edit" as const,
                label: fileName,
                path: fileName,
                linesAdded: a.detail.split("\n").length,
                linesRemoved: 0,
                hunks,
                state: endState,
              };
            }
            if (a.kind === "tool" || a.kind === "skill_read" || a.kind === "command") {
              // `ok` is the backend's own verdict. It outranks anything guessed from the
              // shape of the output text, so it is applied last and never overwritten.
              return { ...a, state: endState, ...stamp(a) };
            }
            return a;
          });
          const stillRunning = afterEnd.some(
            (a) => a.state === "running" && a.id !== "parallel-batch" && a.kind !== "status",
          );
          if (!stillRunning) {
            return afterEnd.map((a) =>
              a.id === "parallel-batch" && a.state === "running"
                ? { ...a, state: "done" as const, label: a.label.replace(/…$/, " — done") }
                : a,
            );
          }
          return afterEnd;
        }
        case "SkillRead":
          return [
            ...activity,
            {
              id: `${d.data.id}-skill-read`,
              kind: "skill_read",
              label: `Read skill ${d.data.category}/${d.data.name}`,
              category: d.data.category,
              name: d.data.name,
              state: "running",
            },
          ];
        case "SkillSaved":
          return [
            ...activity,
            {
              id: `${d.data.id}-skill-save`,
              kind: "skill_save",
              label: `Saved skill ${d.data.category}/${d.data.name}`,
              category: d.data.category,
              name: d.data.name,
              state: "done",
            },
          ];
        case "Command": {
          const idx = activity.findIndex((a) => a.id === d.data.id);
          if (idx >= 0) {
            const next = [...activity];
            next[idx] = {
              ...next[idx],
              kind: "command",
              label: `Run on ${d.data.vps}`,
              detail: d.data.command,
            };
            return next;
          }
          return [
            ...activity,
            {
              id: d.data.id,
              kind: "command",
              label: `Run on ${d.data.vps}`,
              detail: d.data.command,
              state: "running",
            },
          ];
        }
        default:
          return activity;
      }
    }
    default:
      return activity;
  }
}

/** Injectable clock: tests need a duration that does not depend on how fast they run. */
let clock: () => number = () => Date.now();

/** Freeze time for a test. Returns the previous clock so it can be put back. */
export function setActivityClock(fn: () => number): () => number {
  const previous = clock;
  clock = fn;
  return previous;
}

function now(): number {
  return clock();
}

/** The end stamp and the duration it implies, for an item that was started. */
function stamp(item: AgentActivityItem): Partial<AgentActivityItem> {
  const endedAt = now();
  if (item.endedAt) return {};
  return {
    endedAt,
    durationMs: item.startedAt ? Math.max(0, endedAt - item.startedAt) : undefined,
  };
}

/**
 * `exit_code: 3` on the first line of a command result.
 *
 * The backend already formats it; the UI used to throw it away and show only whether the
 * text happened to start with the word "error", so a command that failed with a real
 * exit code looked exactly like one that succeeded.
 */
const EXIT_CODE = /^exit_code:\s*(-?\d+)/m;

/** What `truncate_output` appends when it cuts a result short. */
const TRUNCATED = /\[Output truncated/;

/**
 * A tool result read as a result, rather than as a string that might begin with "error".
 *
 * `startsWith("error")` was the old test, and it is wrong in both directions: a command
 * whose own output begins with the word "error" was reported as a failed tool call, and
 * a command that exited 1 with a normal-looking message was reported as a success. Where
 * the event carries a real verdict — an exit code, or the `ok` flag on ToolEnd — that is
 * what decides; the prefix is only a last resort for tools that return neither.
 */
function outcomeOf(item: AgentActivityItem, output: string): Partial<AgentActivityItem> {
  const exit = EXIT_CODE.exec(output);
  const exitCode = exit ? Number(exit[1]) : undefined;
  const failed =
    exitCode !== undefined
      ? exitCode !== 0
      : // No exit code to go on. Anchored and punctuated, so a log line that merely
        // mentions an error is not mistaken for the tool having failed.
        /^error[:\s]/i.test(output);
  return {
    output,
    exitCode,
    truncated: TRUNCATED.test(output),
    state: failed ? "error" : "done",
    ...stamp(item),
  };
}

/**
 * Mark the tool that is blocked on this command as waiting for a person.
 *
 * A command held at the safety gate emits nothing further: no result, no end event. It
 * kept the same spinner as a tool that was working, so a turn waiting on an approval
 * card looked identical to one making progress, and the only way to find out was to
 * notice the card. Matched on the command text because that is what the approval
 * carries.
 */
export function markAwaitingApproval(
  segments: TurnSegment[],
  command: string,
): TurnSegment[] {
  const needle = command.trim();
  if (!needle) return segments;
  let changed = false;
  const next = segments.map((seg) => {
    if (seg.type !== "activity") return seg;
    const items = seg.items.map((item) => {
      if (item.state !== "running") return item;
      if (!matchesCommand(item, needle)) return item;
      changed = true;
      return { ...item, state: "awaiting_approval" as const };
    });
    return changed ? { type: "activity" as const, items } : seg;
  });
  return changed ? next : segments;
}

/** Put an item that was waiting back to running, once the person has answered. */
export function clearAwaitingApproval(segments: TurnSegment[]): TurnSegment[] {
  let changed = false;
  const next = segments.map((seg) => {
    if (seg.type !== "activity") return seg;
    const items = seg.items.map((item) => {
      if (item.state !== "awaiting_approval") return item;
      changed = true;
      return { ...item, state: "running" as const };
    });
    return changed ? { type: "activity" as const, items } : seg;
  });
  return changed ? next : segments;
}

function matchesCommand(item: AgentActivityItem, command: string): boolean {
  if (item.detail && item.detail.trim() === command) return true;
  const args = item.arguments as { command?: unknown } | undefined;
  return typeof args?.command === "string" && args.command.trim() === command;
}
