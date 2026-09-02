import type { AgentLogEntry, AgentMessage } from "../../../src/lib/tauri";
import { parseTs } from "./channels";

/**
 * What a thread hangs off.
 *
 * Two id spaces, one column. A correction is worth as much attached to a specific
 * action ("that kubectl call hit the wrong context") as to a specific sentence, so
 * `parent_id` names either an `agent_message` or an `agent_log` row.
 */
export type ThreadParent =
  | { kind: "message"; id: string; message: AgentMessage }
  | { kind: "log"; id: string; entry: AgentLogEntry };

/** The two maps a parent is looked up in. */
export type ThreadIndex = {
  messages: Map<string, AgentMessage>;
  logs: Map<string, AgentLogEntry>;
};

export function indexById<T extends { id: string }>(rows: T[]): Map<string, T> {
  return new Map(rows.map((r) => [r.id, r]));
}

export function buildIndex(messages: AgentMessage[], logs: AgentLogEntry[]): ThreadIndex {
  return { messages: indexById(messages), logs: indexById(logs) };
}

/** Find what a reply answers, in either map. Null when it is nowhere. */
export function resolveParent(
  parentId: string | null | undefined,
  index: ThreadIndex,
): ThreadParent | null {
  if (!parentId) return null;
  const message = index.messages.get(parentId);
  if (message) return { kind: "message", id: parentId, message };
  const entry = index.logs.get(parentId);
  if (entry) return { kind: "log", id: parentId, entry };
  return null;
}

/**
 * What the room shows: top-level messages, plus any reply whose parent is nowhere.
 *
 * An orphan degrades to a top-level post rather than disappearing. A reply can name a
 * live log line that has not been written to `agent_log` yet, or one pruned since, and
 * a correction the user typed must never become invisible because the thing it was
 * about scrolled out of the window.
 */
export function threadRoots(messages: AgentMessage[], index: ThreadIndex): AgentMessage[] {
  return messages.filter((m) => !m.parent_id || !resolveParent(m.parent_id, index));
}

/** Everything hanging off one parent, oldest first. */
export function repliesTo(parentId: string, messages: AgentMessage[]): AgentMessage[] {
  return messages
    .filter((m) => m.parent_id === parentId)
    .sort((a, b) => {
      const ta = parseTs(a.created_at);
      const tb = parseTs(b.created_at);
      if (Number.isFinite(ta) && Number.isFinite(tb) && ta !== tb) return ta - tb;
      return a.id.localeCompare(b.id);
    });
}

/**
 * How many replies each parent has, so a root can show "3 replies" without a scan per
 * message. Counted over every reply, including orphans, which is why the map may hold
 * ids that are not in the feed.
 */
export function replyCounts(messages: AgentMessage[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const m of messages) {
    if (!m.parent_id) continue;
    out.set(m.parent_id, (out.get(m.parent_id) ?? 0) + 1);
  }
  return out;
}

/** A one-line summary of the parent, for the thread drawer's header. */
export function parentSummary(parent: ThreadParent): string {
  const raw =
    parent.kind === "message"
      ? parent.message.body
      : [parent.entry.tool, parent.entry.detail].filter(Boolean).join(" ");
  const flat = raw.replace(/\s+/g, " ").trim();
  return flat.length > 140 ? `${flat.slice(0, 139)}…` : flat;
}
