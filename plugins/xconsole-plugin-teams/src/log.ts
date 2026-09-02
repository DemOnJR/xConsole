import type { AgentLogEntry } from "../../../src/lib/tauri";
import { parseTs } from "./channels";

/**
 * A status event as it arrived, tagged with when.
 *
 * `agent://persona-status` carries no id and is overwritten in the store, so the log
 * channel keeps its own tail: the store answers "what is Ada doing", this answers "what
 * has Ada been doing".
 */
export type LiveLogLine = {
  personaId: string;
  status: string;
  tool?: string | null;
  detail: string;
  /** Milliseconds, from when the event arrived. */
  at: number;
};

/** One line in a log channel, from either source. */
export type LogLine = {
  /** A row id when it is persisted; a session-local `live:` id until then. */
  id: string;
  personaId: string;
  status: string;
  tool: string | null;
  detail: string;
  at: number;
  /**
   * True while this line exists only in the live feed.
   *
   * A live line can still be replied to. Its parent id does not survive a restart, and
   * the reply then shows as a top-level post in the same log channel rather than
   * vanishing -- see `threadRoots`.
   */
  live: boolean;
  /** How many identical consecutive lines this stands for. 1 for a single line. */
  repeat: number;
};

/**
 * How close two lines must be to be the same event arriving twice.
 *
 * A line reaches the channel by two paths: the live event as it happens, and the
 * `agent_log` row on the next reload. They carry different ids and slightly different
 * timestamps, so identity has to be the content plus a window.
 */
const SAME_EVENT_MS = 5000;

/**
 * How far apart two identical lines can be and still be one retry loop.
 *
 * Unbounded collapsing is wrong in the other direction: the same command run once in
 * the morning and once in the evening, with nothing logged between, would read as a
 * single line stamped with the morning -- which is worse than the noise it removed.
 */
const COLLAPSE_GAP_MS = 120_000;

function norm(s: string | null | undefined): string {
  return (s ?? "").trim();
}

function sameContent(a: { personaId: string; status: string; tool: string | null; detail: string },
                     b: { personaId: string; status: string; tool: string | null; detail: string }): boolean {
  return (
    a.personaId === b.personaId &&
    a.status === b.status &&
    a.tool === b.tool &&
    a.detail === b.detail
  );
}

function fromEntry(e: AgentLogEntry): LogLine {
  const at = parseTs(e.created_at);
  return {
    id: e.id,
    personaId: e.persona_id,
    status: norm(e.status) || "working",
    tool: norm(e.tool) || null,
    detail: norm(e.detail),
    at: Number.isFinite(at) ? at : 0,
    live: false,
    repeat: 1,
  };
}

function fromLive(l: LiveLogLine): LogLine {
  return {
    id: `live:${l.personaId}:${l.at}`,
    personaId: l.personaId,
    status: norm(l.status) || "working",
    tool: norm(l.tool) || null,
    detail: norm(l.detail),
    at: l.at,
    live: true,
    repeat: 1,
  };
}

/**
 * The log channel's contents: persisted rows and the live tail, in one ordered list.
 *
 * Two things this must not do. It must not show the same action twice because it
 * arrived by both paths, and it must not fill the channel with forty identical
 * `run_command` lines when an agent retries -- consecutive identical lines collapse into
 * one carrying a count, which keeps the interesting line visible instead of scrolled
 * away.
 *
 * A collapsed line keeps the id of its first occurrence, so a thread opened on it stays
 * attached to the same anchor as more repeats arrive.
 */
export function mergeLog(persisted: AgentLogEntry[], live: LiveLogLine[]): LogLine[] {
  const rows = persisted.map(fromEntry);
  const kept: LogLine[] = [...rows];
  for (const l of live) {
    const line = fromLive(l);
    const already = rows.some(
      (r) => sameContent(r, line) && Math.abs(r.at - line.at) <= SAME_EVENT_MS,
    );
    if (already) continue;
    // A live event repeated at the same millisecond is the same event, not two.
    if (kept.some((k) => k.live && k.id === line.id)) continue;
    kept.push(line);
  }

  kept.sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));

  const out: LogLine[] = [];
  // The previous line's own timestamp, not the collapsed line's, so a long run of
  // retries stays one line for as long as they keep coming.
  let prevAt = 0;
  for (const line of kept) {
    const last = out[out.length - 1];
    if (last && sameContent(last, line) && line.at - prevAt <= COLLAPSE_GAP_MS) {
      prevAt = line.at;
      last.repeat += 1;
      // A persisted anchor beats a live one: it survives a restart.
      if (last.live && !line.live) {
        last.id = line.id;
        last.live = false;
      }
      continue;
    }
    prevAt = line.at;
    out.push({ ...line });
  }
  return out;
}

/** What a line reads as in the feed. */
export function logText(line: LogLine): string {
  const parts = [line.tool, line.detail].filter((s): s is string => Boolean(s));
  const body = parts.join(" ") || line.status;
  return line.repeat > 1 ? `${body} (x${line.repeat})` : body;
}
