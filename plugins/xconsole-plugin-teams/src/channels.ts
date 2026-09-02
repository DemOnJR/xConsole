import type { AgentMessage, GoalSession, Persona, Workspace } from "../../../src/lib/tauri";

export const COMPANY = "__company__";

export type ChannelKind = "company" | "project" | "team" | "dm" | "log";

export type Channel = {
  /** Selection key, and the legacy identity the original derivation was keyed on. */
  id: string;
  /**
   * The persisted room id, in the grammar below.
   *
   * Membership used to be inferred from `(workspace_id, from_id, to_id)`, which cannot
   * tell `#general` from `#log-ada` inside one project: both are "a message on this
   * project". Per-agent rooms are unrepresentable without an id written on the row.
   */
  channelId: string;
  kind: ChannelKind;
  slug: string;
  title: string;
  subtitle: string;
  workspaceId: string | null;
  leadId: string | null;
  /** Whose log this is, for `log` channels and DMs. Null everywhere else. */
  personaId: string | null;
  memberIds: string[];
};

/**
 * A channel id, taken apart.
 *
 * The grammar is a plain string rather than a channel table: the set of rooms is a pure
 * function of the projects and agents that already exist, so a table would be a second
 * copy of that to keep in sync, and every project created or deleted would need a
 * matching room migration.
 */
export type ChannelRef =
  | { kind: "company" }
  | { kind: "project"; workspaceId: string }
  | { kind: "log"; workspaceId: string; personaId: string }
  | { kind: "team"; leadId: string }
  | { kind: "dm"; personaId: string };

/** Build the persisted room id for a channel. Must match `parse_channel` in Rust. */
export function channelIdFor(ref: ChannelRef): string {
  switch (ref.kind) {
    case "company":
      return "company";
    case "project":
      return `ws:${ref.workspaceId}:general`;
    case "log":
      return `ws:${ref.workspaceId}:log:${ref.personaId}`;
    case "team":
      return `team:${ref.leadId}`;
    case "dm":
      return `dm:${ref.personaId}`;
  }
}

/** Take a room id apart, or refuse it. Anything not in the grammar is not a room. */
export function parseChannelId(id: string): ChannelRef | null {
  const parts = (id || "").trim().split(":");
  const filled = (s: string | undefined): s is string => Boolean(s && s.trim());
  if (parts.length === 1 && parts[0] === "company") return { kind: "company" };
  if (parts.length === 3 && parts[0] === "ws" && parts[2] === "general" && filled(parts[1])) {
    return { kind: "project", workspaceId: parts[1] };
  }
  if (
    parts.length === 4 &&
    parts[0] === "ws" &&
    parts[2] === "log" &&
    filled(parts[1]) &&
    filled(parts[3])
  ) {
    return { kind: "log", workspaceId: parts[1], personaId: parts[3] };
  }
  if (parts.length === 2 && parts[0] === "team" && filled(parts[1])) {
    return { kind: "team", leadId: parts[1] };
  }
  if (parts.length === 2 && parts[0] === "dm" && filled(parts[1])) {
    return { kind: "dm", personaId: parts[1] };
  }
  return null;
}

/**
 * Milliseconds for a timestamp, or NaN.
 *
 * SQLite writes `datetime('now')` as "2026-09-02 15:16:00" with no zone, which a
 * browser reads as local time while the value is UTC. Every cursor comparison here
 * would then be wrong by the offset, so the shape is normalised before parsing.
 */
export function parseTs(iso?: string | null): number {
  if (!iso) return NaN;
  const s = iso.trim();
  const normal = /^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(\.\d+)?$/.test(s)
    ? `${s.replace(" ", "T")}Z`
    : s;
  return Date.parse(normal);
}

export function slugify(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return s || "channel";
}

/** Lead + everyone who reports to them, transitively. */
export function reportsTree(personas: Persona[], leadId: string): Persona[] {
  const byId = new Map(personas.map((p) => [p.id, p]));
  const out: Persona[] = [];
  const seen = new Set<string>();
  const walk = (id: string) => {
    if (seen.has(id)) return;
    seen.add(id);
    const p = byId.get(id);
    if (!p) return;
    out.push(p);
    for (const child of personas) {
      if (child.reports_to === id) walk(child.id);
    }
  };
  walk(leadId);
  return out;
}

export function vpsIdsOfWorkspace(ws: Workspace): string[] {
  try {
    const raw = JSON.parse(ws.nodes_json || "{}");
    const list: { vpsId?: string }[] = Array.isArray(raw) ? raw : raw.nodes || [];
    return list.map((n) => n.vpsId).filter((id): id is string => Boolean(id));
  } catch {
    return [];
  }
}

/**
 * Who belongs on a project channel.
 *
 * Assigned `workspace_id` wins. When nobody was stamped onto the project (the
 * usual case: hired company-wide, then sent to work on K8S), infer from goals
 * on that workspace and from the persona's default servers matching the
 * project's terminals.
 */
export function membersForProject(
  personas: Persona[],
  ws: Workspace,
  goals: GoalSession[],
): Persona[] {
  const assigned = personas.filter((p) => p.workspace_id === ws.id);
  if (assigned.length) return assigned;
  const fromGoals = new Set(
    goals.filter((g) => g.workspace_id === ws.id && g.persona_id).map((g) => g.persona_id as string),
  );
  if (fromGoals.size) return personas.filter((p) => fromGoals.has(p.id));
  const vps = new Set(vpsIdsOfWorkspace(ws));
  const byTarget = personas.filter((p) => p.targets.some((t) => vps.has(t)));
  // Everyone's defaults often list every server; that is not a team.
  if (byTarget.length > 0 && byTarget.length < personas.length) return byTarget;
  return [];
}

function teamSlug(lead: Persona): string {
  const blob = `${lead.name} ${lead.role}`.toLowerCase();
  if (/\bcsb\b|counter-strike-boost/.test(blob)) return "csb";
  return slugify(lead.name);
}

export function buildChannels(
  personas: Persona[],
  workspaces: Workspace[],
  goals: GoalSession[],
): Channel[] {
  const channels: Channel[] = [
    {
      id: COMPANY,
      channelId: channelIdFor({ kind: "company" }),
      kind: "company",
      slug: "company",
      title: "company",
      subtitle: "Everyone",
      workspaceId: null,
      leadId: null,
      personaId: null,
      memberIds: personas.map((p) => p.id),
    },
  ];

  const projects = [...workspaces].sort((a, b) => a.name.localeCompare(b.name));
  for (const ws of projects) {
    const members = membersForProject(personas, ws, goals);
    channels.push({
      id: ws.id,
      channelId: channelIdFor({ kind: "project", workspaceId: ws.id }),
      kind: "project",
      slug: slugify(ws.name),
      title: ws.name,
      subtitle: members.length === 1 ? "1 on this project" : `${members.length} on this project`,
      workspaceId: ws.id,
      leadId: null,
      personaId: null,
      memberIds: members.map((p) => p.id),
    });
  }

  const leads = personas.filter((p) => !p.reports_to);
  for (const lead of leads) {
    const team = reportsTree(personas, lead.id);
    if (team.length <= 1) continue;
    const slug = teamSlug(lead);
    channels.push({
      id: `team:${lead.id}`,
      channelId: channelIdFor({ kind: "team", leadId: lead.id }),
      kind: "team",
      slug,
      title: slug,
      subtitle: `${lead.name}'s team`,
      workspaceId: lead.workspace_id || null,
      leadId: lead.id,
      personaId: null,
      memberIds: team.map((p) => p.id),
    });
  }

  return channels;
}

export function dmChannels(personas: Persona[]): Channel[] {
  return [...personas]
    .sort((a, b) => Number(b.enabled) - Number(a.enabled) || a.name.localeCompare(b.name))
    .map((p) => ({
      id: `dm:${p.id}`,
      channelId: channelIdFor({ kind: "dm", personaId: p.id }),
      kind: "dm" as const,
      slug: slugify(p.name),
      title: p.name,
      subtitle: p.role || "Direct message",
      workspaceId: p.workspace_id || null,
      leadId: null,
      personaId: p.id,
      memberIds: [p.id],
    }));
}

/**
 * A project, as a server: one tile on the rail and the rooms behind it.
 *
 * Company-wide talk and direct messages sit outside every guild, because they are not
 * about one project and burying them inside whichever project was open would make them
 * unfindable from the others.
 */
export type Guild = {
  /** `COMPANY`, or the workspace id. */
  id: string;
  kind: "company" | "project";
  name: string;
  workspaceId: string | null;
  channels: Channel[];
};

/** One agent's live log, inside the project they are working on. */
function logChannel(ws: Workspace, p: Persona): Channel {
  const slug = `log-${slugify(p.name)}`;
  return {
    id: channelIdFor({ kind: "log", workspaceId: ws.id, personaId: p.id }),
    channelId: channelIdFor({ kind: "log", workspaceId: ws.id, personaId: p.id }),
    kind: "log",
    slug,
    title: slug,
    subtitle: `What ${p.name} is doing`,
    workspaceId: ws.id,
    leadId: null,
    personaId: p.id,
    memberIds: [p.id],
  };
}

/**
 * The rail, and what is behind each tile.
 *
 * A team room follows its lead: a lead assigned to a project belongs inside that
 * project, and a lead who answers across all of them stays at company level rather than
 * appearing under one project they happen not to work on.
 *
 * Log channels exist only inside a project, because a log id names one. An agent with no
 * project of its own gets one as soon as it picks up work there -- `membersForProject`
 * infers membership from goals -- and is reachable by DM until then.
 */
export function buildGuilds(
  personas: Persona[],
  workspaces: Workspace[],
  goals: GoalSession[],
): Guild[] {
  const flat = buildChannels(personas, workspaces, goals);
  const teams = flat.filter((c) => c.kind === "team");
  const guilds: Guild[] = [
    {
      id: COMPANY,
      kind: "company",
      name: "Company",
      workspaceId: null,
      channels: [
        ...flat.filter((c) => c.kind === "company"),
        ...teams.filter((t) => !t.workspaceId),
      ],
    },
  ];

  const byId = new Map(personas.map((p) => [p.id, p]));
  for (const ws of [...workspaces].sort((a, b) => a.name.localeCompare(b.name))) {
    const general = flat.find((c) => c.kind === "project" && c.id === ws.id);
    if (!general) continue;
    const members = general.memberIds
      .map((id) => byId.get(id))
      .filter((p): p is Persona => Boolean(p));
    guilds.push({
      id: ws.id,
      kind: "project",
      name: ws.name,
      workspaceId: ws.id,
      channels: [
        general,
        ...teams.filter((t) => t.workspaceId === ws.id),
        ...members.map((p) => logChannel(ws, p)),
      ],
    });
  }
  return guilds;
}

/** Every room in every guild, flattened, for lookup by id. */
export function allGuildChannels(guilds: Guild[]): Channel[] {
  return guilds.flatMap((g) => g.channels);
}

/**
 * Does this message belong in this room?
 *
 * A stamped `channel_id` is the answer, full stop. Everything else falls back to the
 * original derivation, because roughly five hundred messages already exist with no
 * channel on them and an upgrade that hid all of them would read as data loss.
 */
export function messageInChannel(msg: AgentMessage, ch: Channel): boolean {
  if (msg.channel_id) return msg.channel_id === ch.channelId;
  // A log channel is unrepresentable in the legacy shape -- "a message on this project"
  // cannot distinguish #general from #log-ada -- so nothing legacy routes into one.
  if (ch.kind === "log") return false;
  if (ch.kind === "dm") {
    const pid = ch.memberIds[0];
    return msg.from_id === pid || msg.to_id === pid;
  }
  if (ch.kind === "project") {
    return msg.workspace_id === ch.id;
  }
  if (ch.kind === "team") {
    const ids = new Set(ch.memberIds);
    return (msg.from_id ? ids.has(msg.from_id) : false) || (msg.to_id ? ids.has(msg.to_id) : false);
  }
  return !msg.workspace_id;
}

export type MessageGroup = {
  key: string;
  fromId: string | null;
  toId: string | null;
  kind: string;
  messages: AgentMessage[];
};

const GROUP_GAP_MS = 5 * 60 * 1000;

export function groupMessages(messages: AgentMessage[]): MessageGroup[] {
  const groups: MessageGroup[] = [];
  for (const m of messages) {
    const last = groups[groups.length - 1];
    const prev = last?.messages[last.messages.length - 1];
    const samePerson = last && last.fromId === (m.from_id ?? null) && last.toId === (m.to_id ?? null) && last.kind === m.kind;
    const close = prev && closeInTime(prev, m);
    if (samePerson && close) {
      last.messages.push(m);
    } else {
      groups.push({
        key: m.id,
        fromId: m.from_id ?? null,
        toId: m.to_id ?? null,
        kind: m.kind,
        messages: [m],
      });
    }
  }
  return groups;
}

function closeInTime(a: AgentMessage, b: AgentMessage): boolean {
  const ta = Date.parse(a.created_at || "");
  const tb = Date.parse(b.created_at || "");
  if (!Number.isFinite(ta) || !Number.isFinite(tb)) return true;
  return Math.abs(tb - ta) <= GROUP_GAP_MS;
}

export function dayKey(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

export function dayLabel(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

const AVATAR = ["#7c9cbf", "#8fbf9f", "#c4a35a", "#c47c7c", "#9b8fc4", "#7cb3c4", "#c49b7c"];

export function avatarColor(idOrName: string): string {
  let h = 0;
  for (let i = 0; i < idOrName.length; i++) h = (h * 31 + idOrName.charCodeAt(i)) | 0;
  return AVATAR[Math.abs(h) % AVATAR.length];
}

export function nameOf(personas: Persona[], id?: string | null): string {
  if (!id) return "You";
  return personas.find((p) => p.id === id)?.name || "Unknown";
}
