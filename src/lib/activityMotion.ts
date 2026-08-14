import type { AgentActivityItem } from "../stores/agentStore";

/**
 * Line-morph loops. Each one rearranges the four strokes of a # into
 * figures and back. Opacity/transform only — no filter or canvas.
 *
 *   weave  # → + → × → + → #
 *   scan   # → = → shifted = → #
 *   stack  # → four text-lines → #
 *   fold   # → box → diamond → box → #
 *   gate   # → || → + → || → #
 *   burst  # → * → # → *
 *   check  # → = → ✓ → #
 *   link   # → box → = → box → #
 *   cycle  # → + → = → || → × → #
 *   break  # → strokes fly apart → #
 */
export const HASH_MOTIONS = [
  "weave",
  "scan",
  "stack",
  "fold",
  "gate",
  "burst",
  "check",
  "link",
  "cycle",
  "break",
] as const;

export type HashMotion = (typeof HASH_MOTIONS)[number];

export type ActivityKind =
  | "think"
  | "read"
  | "write"
  | "edit"
  | "exec"
  | "search"
  | "todo"
  | "connect"
  | "work"
  | "error";

const KIND_MOTION: Record<ActivityKind, HashMotion> = {
  think: "weave",
  read: "scan",
  write: "stack",
  edit: "fold",
  exec: "gate",
  search: "burst",
  todo: "check",
  connect: "link",
  work: "cycle",
  error: "break",
};

export function motionForKind(kind: ActivityKind): HashMotion {
  return KIND_MOTION[kind];
}

export function activityKind(item?: Pick<AgentActivityItem, "kind" | "tool" | "label" | "state" | "path"> | null): ActivityKind {
  if (!item) return "think";
  if (item.state === "error") return "error";
  const tool = (item.tool || "").toLowerCase();
  const label = item.label.trim();
  if (item.kind === "file_edit" || tool === "write_file" || /^write /i.test(label)) return "write";
  if (tool === "edit_file" || tool === "local_edit_file" || /^edit /i.test(label)) return "edit";
  if (tool === "read_file" || /^read /i.test(label) || label.startsWith("Read file")) return "read";
  if (tool === "grep_search" || tool === "local_grep_search" || /^search/i.test(label)) return "search";
  if (tool === "todo_write" || /^update checklist$/i.test(label)) return "todo";
  if (tool === "canvas_open_terminal" || tool === "canvas_refresh") return "connect";
  if (
    tool === "run_command" ||
    tool === "shell" ||
    tool === "terminal_send" ||
    item.kind === "command" ||
    /^run on /i.test(label) ||
    label.startsWith("SSH ›") ||
    label.startsWith("Shell ›")
  ) {
    return "exec";
  }
  if (item.kind === "status") return "work";
  return "work";
}
