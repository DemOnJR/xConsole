import type { AgentMessage, ChannelUnread } from "../../../src/lib/tauri";
import { parseTs, type Guild } from "./channels";

/** What a badge shows: how much is new, and how much of it is addressed to you. */
export type UnreadCount = { count: number; mentions: number };

/** Where each reader's cursor sits in each room. Missing = never opened. */
export type ReadCursors = Record<string, string | null | undefined>;

export const NOTHING_UNREAD: UnreadCount = { count: 0, mentions: 0 };

/** Cursors out of what the backend returned, so counting can continue client-side. */
export function cursorsFrom(rows: ChannelUnread[]): ReadCursors {
  const out: ReadCursors = {};
  for (const r of rows) out[r.channel_id] = r.last_read_at ?? null;
  return out;
}

/**
 * Is this message new to `selfId` (null = the user)?
 *
 * A message with an unreadable timestamp counts as new. Over-notifying is a nuisance;
 * silently swallowing a message because its timestamp did not parse is the failure that
 * makes people stop trusting the badge.
 */
function isNew(m: AgentMessage, cursor: string | null | undefined): boolean {
  if (!cursor) return true;
  const at = parseTs(m.created_at);
  const seen = parseTs(cursor);
  if (!Number.isFinite(at) || !Number.isFinite(seen)) return true;
  return at > seen;
}

function isOwn(m: AgentMessage, selfId: string | null): boolean {
  return (m.from_id ?? null) === selfId;
}

/**
 * How much of one room `selfId` has not seen.
 *
 * Your own words never count: a badge that lights up because you just spoke trains
 * people to ignore it. Only rows carrying a `channel_id` are counted -- legacy messages
 * predate rooms and were never tracked against a cursor, so counting them would show
 * every old conversation as unread once on upgrade.
 */
export function unreadInChannel(
  messages: AgentMessage[],
  channelId: string,
  cursor: string | null | undefined,
  selfId: string | null,
): UnreadCount {
  let count = 0;
  let mentions = 0;
  for (const m of messages) {
    if (m.channel_id !== channelId) continue;
    if (isOwn(m, selfId)) continue;
    if (!isNew(m, cursor)) continue;
    count += 1;
    if (selfId && (m.mentions ?? []).includes(selfId)) mentions += 1;
  }
  return { count, mentions };
}

/** Every room that has something new, keyed by channel id. */
export function unreadByChannel(
  messages: AgentMessage[],
  cursors: ReadCursors,
  selfId: string | null,
): Record<string, UnreadCount> {
  const out: Record<string, UnreadCount> = {};
  for (const m of messages) {
    const ch = m.channel_id;
    if (!ch) continue;
    if (isOwn(m, selfId)) continue;
    if (!isNew(m, cursors[ch])) continue;
    const at = out[ch] ?? { count: 0, mentions: 0 };
    at.count += 1;
    if (selfId && (m.mentions ?? []).includes(selfId)) at.mentions += 1;
    out[ch] = at;
  }
  return out;
}

/** What the rail tile shows: everything unread behind that server, summed. */
export function unreadForGuild(
  guild: Guild,
  byChannel: Record<string, UnreadCount>,
): UnreadCount {
  let count = 0;
  let mentions = 0;
  for (const ch of guild.channels) {
    const at = byChannel[ch.channelId];
    if (!at) continue;
    count += at.count;
    mentions += at.mentions;
  }
  return { count, mentions };
}

/** Badge text. Past 99 the exact number stops meaning anything. */
export function badge(n: number): string {
  if (n <= 0) return "";
  return n > 99 ? "99+" : String(n);
}

/**
 * Persona ids named with `@` in a draft.
 *
 * Matched against the real staff list rather than by pattern, so `@sha256` in a paste is
 * not a mention, and longest-name-first so `@Ada Lovelace` does not resolve to `@Ada`.
 */
export function mentionIds(body: string, personas: { id: string; name: string }[]): string[] {
  const text = body.toLowerCase();
  const byLongest = [...personas].sort((a, b) => b.name.length - a.name.length);
  const out: string[] = [];
  const taken: [number, number][] = [];
  for (const p of byLongest) {
    const needle = `@${p.name.toLowerCase()}`;
    let from = 0;
    for (;;) {
      const at = text.indexOf(needle, from);
      if (at < 0) break;
      from = at + needle.length;
      // A longer name already claimed this span.
      if (taken.some(([s, e]) => at >= s && at < e)) continue;
      taken.push([at, from]);
      if (!out.includes(p.id)) out.push(p.id);
      break;
    }
  }
  return out;
}
