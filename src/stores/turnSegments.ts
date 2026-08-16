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
          state: "running",
        },
      ];
    case "ToolResult": {
      if (ev.data.id.startsWith("snapshot-")) return activity;
      const idx = activity.findIndex((a) => a.id === ev.data.id);
      let next = activity;
      if (idx >= 0) {
        next = [...activity];
        next[idx] = {
          ...next[idx],
          output: ev.data.output,
          state: (ev.data.output.startsWith("error") ? "error" : "done") as "error" | "done",
        };
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
              return { ...a, state: endState };
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
              return { ...a, state: endState };
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
