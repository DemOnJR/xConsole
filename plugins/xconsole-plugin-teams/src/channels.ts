import type { AgentMessage, GoalSession, Persona, Workspace } from "../../../src/lib/tauri";

export const COMPANY = "__company__";

export type ChannelKind = "company" | "project" | "team" | "dm";

export type Channel = {
  id: string;
  kind: ChannelKind;
  slug: string;
  title: string;
  subtitle: string;
  workspaceId: string | null;
  leadId: string | null;
  memberIds: string[];
};

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
      kind: "company",
      slug: "company",
      title: "company",
      subtitle: "Everyone",
      workspaceId: null,
      leadId: null,
      memberIds: personas.map((p) => p.id),
    },
  ];

  const projects = [...workspaces].sort((a, b) => a.name.localeCompare(b.name));
  for (const ws of projects) {
    const members = membersForProject(personas, ws, goals);
    channels.push({
      id: ws.id,
      kind: "project",
      slug: slugify(ws.name),
      title: ws.name,
      subtitle: members.length === 1 ? "1 on this project" : `${members.length} on this project`,
      workspaceId: ws.id,
      leadId: null,
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
      kind: "team",
      slug,
      title: slug,
      subtitle: `${lead.name}'s team`,
      workspaceId: lead.workspace_id || null,
      leadId: lead.id,
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
      kind: "dm" as const,
      slug: slugify(p.name),
      title: p.name,
      subtitle: p.role || "Direct message",
      workspaceId: p.workspace_id || null,
      leadId: null,
      memberIds: [p.id],
    }));
}

export function messageInChannel(msg: AgentMessage, ch: Channel): boolean {
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
