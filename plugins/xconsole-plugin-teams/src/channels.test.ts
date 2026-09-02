import { describe, expect, it } from "vitest";
import type { AgentMessage, GoalSession, Persona, Workspace } from "../../../src/lib/tauri";
import {
  allGuildChannels,
  avatarColor,
  buildChannels,
  buildGuilds,
  channelIdFor,
  COMPANY,
  dmChannels,
  groupMessages,
  initials,
  membersForProject,
  messageInChannel,
  nameOf,
  parseChannelId,
  parseTs,
  reportsTree,
  slugify,
  vpsIdsOfWorkspace,
  type ChannelRef,
} from "./channels";

const persona = (partial: Partial<Persona> & Pick<Persona, "id" | "name">): Persona => ({
  role: "",
  instructions: "",
  targets: [],
  enabled: true,
  ...partial,
});

const ws = (partial: Partial<Workspace> & Pick<Workspace, "id" | "name">): Workspace => ({
  ...partial,
});

const goal = (partial: Partial<GoalSession> & Pick<GoalSession, "id" | "title" | "status">): GoalSession => ({
  raw_request: "",
  spec_json: "{}",
  kanban_json: "[]",
  memory_json: "{}",
  cycles: 1,
  ...partial,
});

const msg = (partial: Partial<AgentMessage> & Pick<AgentMessage, "id" | "body">): AgentMessage => ({
  kind: "note",
  ...partial,
});

describe("slugify", () => {
  it("turns a project name into a channel slug", () => {
    expect(slugify("K8S")).toBe("k8s");
    expect(slugify("Counter-Strike Boost")).toBe("counter-strike-boost");
  });
});

describe("reportsTree", () => {
  it("includes the lead and everyone under them", () => {
    const ada = persona({ id: "ada", name: "Ada" });
    const bruno = persona({ id: "bruno", name: "Bruno", reports_to: "ada" });
    const quill = persona({ id: "quill", name: "Quill", reports_to: "ada" });
    const adrian = persona({ id: "adrian", name: "Adrian" });
    const tree = reportsTree([ada, bruno, quill, adrian], "ada");
    expect(tree.map((p) => p.name).sort()).toEqual(["Ada", "Bruno", "Quill"]);
  });
});

describe("buildChannels", () => {
  it("lists every project even when nobody is assigned to one", () => {
    const people = [persona({ id: "ada", name: "Ada" })];
    const projects = [ws({ id: "k8s", name: "K8S" }), ws({ id: "m2", name: "Metin2" })];
    const ch = buildChannels(people, projects, []);
    expect(ch.map((c) => c.slug)).toEqual(["company", "k8s", "metin2"]);
    expect(ch.filter((c) => c.kind === "project")).toHaveLength(2);
  });

  it("makes a team channel per lead who has reports, named from the remit", () => {
    const ada = persona({ id: "ada", name: "Ada", role: "Lead / orchestrator" });
    const bruno = persona({ id: "bruno", name: "Bruno", reports_to: "ada" });
    const adrian = persona({
      id: "adrian",
      name: "Adrian",
      role: "Lead of the CSB Team (Counter-Strike-Boost.com)",
    });
    const maria = persona({ id: "maria", name: "Maria", reports_to: "adrian" });
    const ch = buildChannels([ada, bruno, adrian, maria], [], []);
    const teams = ch.filter((c) => c.kind === "team");
    expect(teams.map((t) => t.slug).sort()).toEqual(["ada", "csb"]);
    const csb = teams.find((t) => t.slug === "csb")!;
    expect(csb.memberIds.sort()).toEqual(["adrian", "maria"]);
    expect(csb.subtitle).toBe("Adrian's team");
  });

  it("does not invent a team channel for a lone lead with no reports", () => {
    const ada = persona({ id: "ada", name: "Ada" });
    const ch = buildChannels([ada], [], []);
    expect(ch.filter((c) => c.kind === "team")).toHaveLength(0);
  });
});

describe("membersForProject", () => {
  it("uses workspace_id when it is set", () => {
    const ada = persona({ id: "ada", name: "Ada", workspace_id: "k8s" });
    const bruno = persona({ id: "bruno", name: "Bruno" });
    const got = membersForProject([ada, bruno], ws({ id: "k8s", name: "K8S" }), []);
    expect(got.map((p) => p.id)).toEqual(["ada"]);
  });

  it("infers membership from goals on the project when nobody is assigned", () => {
    const k8s = ws({ id: "k8s", name: "K8S" });
    const ada = persona({ id: "ada", name: "Ada", targets: ["vps-1"] });
    const grace = persona({ id: "grace", name: "Grace" });
    const got = membersForProject(
      [ada, grace],
      k8s,
      [goal({ id: "g", title: "t", status: "done", persona_id: "grace", workspace_id: "k8s" })],
    );
    expect(got.map((p) => p.id)).toEqual(["grace"]);
  });

  it("falls back to matching VPS targets when that is not the whole company", () => {
    const k8s = ws({
      id: "k8s",
      name: "K8S",
      nodes_json: JSON.stringify({ nodes: [{ vpsId: "vps-1" }] }),
    });
    const ada = persona({ id: "ada", name: "Ada", targets: ["vps-1"] });
    const other = persona({ id: "x", name: "X", targets: ["vps-9"] });
    const got = membersForProject([ada, other], k8s, []);
    expect(got.map((p) => p.id)).toEqual(["ada"]);
  });
});

describe("vpsIdsOfWorkspace", () => {
  it("reads vpsId off tiled nodes", () => {
    const ids = vpsIdsOfWorkspace(
      ws({
        id: "k8s",
        name: "K8S",
        nodes_json: JSON.stringify({ nodes: [{ vpsId: "a" }, { vpsId: "" }, { vpsId: "b" }] }),
      }),
    );
    expect(ids).toEqual(["a", "b"]);
  });
});

describe("messageInChannel", () => {
  const ada = persona({ id: "ada", name: "Ada" });
  const bruno = persona({ id: "bruno", name: "Bruno", reports_to: "ada" });
  const ch = buildChannels([ada, bruno], [ws({ id: "k8s", name: "K8S" })], []);
  const company = ch.find((c) => c.kind === "company")!;
  const project = ch.find((c) => c.kind === "project")!;
  const team = ch.find((c) => c.kind === "team")!;
  const dm = dmChannels([ada])[0];

  it("puts unscoped messages in company, not in a project", () => {
    const m = msg({ id: "1", body: "hi", from_id: "ada" });
    expect(messageInChannel(m, company)).toBe(true);
    expect(messageInChannel(m, project)).toBe(false);
  });

  it("puts a project-stamped message only on that project", () => {
    const m = msg({ id: "2", body: "deploy", from_id: "ada", workspace_id: "k8s" });
    expect(messageInChannel(m, project)).toBe(true);
    expect(messageInChannel(m, company)).toBe(false);
  });

  it("shows a team member's mail on the team channel", () => {
    const m = msg({ id: "3", body: "disk", from_id: "bruno", to_id: "ada" });
    expect(messageInChannel(m, team)).toBe(true);
    const outsider = msg({ id: "4", body: "x", from_id: "nobody" });
    expect(messageInChannel(outsider, team)).toBe(false);
  });

  it("scopes a DM to that person at either end", () => {
    expect(messageInChannel(msg({ id: "5", body: "a", from_id: "ada" }), dm)).toBe(true);
    expect(messageInChannel(msg({ id: "6", body: "b", to_id: "ada" }), dm)).toBe(true);
    expect(messageInChannel(msg({ id: "7", body: "c", from_id: "bruno" }), dm)).toBe(false);
  });
});

describe("groupMessages", () => {
  it("merges consecutive messages from the same person", () => {
    const groups = groupMessages([
      msg({ id: "1", body: "a", from_id: "ada", created_at: "2026-09-01T16:00:00Z" }),
      msg({ id: "2", body: "b", from_id: "ada", created_at: "2026-09-01T16:01:00Z" }),
      msg({ id: "3", body: "c", from_id: "bruno", created_at: "2026-09-01T16:02:00Z" }),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0].messages.map((m) => m.id)).toEqual(["1", "2"]);
    expect(groups[1].fromId).toBe("bruno");
  });
});

describe("initials / nameOf / avatarColor", () => {
  it("uses two letters from a single name", () => {
    expect(initials("Ada")).toBe("AD");
    expect(initials("You")).toBe("YO");
  });
  it("names the user when from_id is empty", () => {
    expect(nameOf([persona({ id: "ada", name: "Ada" })], null)).toBe("You");
    expect(nameOf([persona({ id: "ada", name: "Ada" })], "ada")).toBe("Ada");
  });
  it("is stable for the same id", () => {
    expect(avatarColor("ada")).toBe(avatarColor("ada"));
  });
});

describe("channelIdFor / parseChannelId", () => {
  it("round-trips every room in the grammar", () => {
    const refs: ChannelRef[] = [
      { kind: "company" },
      { kind: "project", workspaceId: "k8s" },
      { kind: "log", workspaceId: "k8s", personaId: "ada" },
      { kind: "team", leadId: "ada" },
      { kind: "dm", personaId: "ada" },
    ];
    for (const ref of refs) {
      expect(parseChannelId(channelIdFor(ref))).toEqual(ref);
    }
  });

  it("writes the ids the Rust side parses", () => {
    expect(channelIdFor({ kind: "company" })).toBe("company");
    expect(channelIdFor({ kind: "project", workspaceId: "k8s" })).toBe("ws:k8s:general");
    expect(channelIdFor({ kind: "log", workspaceId: "k8s", personaId: "ada" })).toBe(
      "ws:k8s:log:ada",
    );
    expect(channelIdFor({ kind: "team", leadId: "ada" })).toBe("team:ada");
    expect(channelIdFor({ kind: "dm", personaId: "ada" })).toBe("dm:ada");
  });

  it("refuses anything outside the grammar rather than inventing a room", () => {
    for (const bad of [
      "",
      "ws",
      "ws:",
      "ws:k8s",
      "ws::general",
      "ws:k8s:general:extra",
      "ws:k8s:log",
      "ws:k8s:log:",
      "ws:k8s:logs:ada",
      "team:",
      "dm:",
      "Company",
      "general",
    ]) {
      expect(parseChannelId(bad)).toBeNull();
    }
  });
});

describe("buildGuilds", () => {
  const ada = persona({ id: "ada", name: "Ada", workspace_id: "k8s" });
  const bruno = persona({ id: "bruno", name: "Bruno", workspace_id: "k8s", reports_to: "ada" });
  const zoe = persona({ id: "zoe", name: "Zoe" });
  const k8s = ws({ id: "k8s", name: "K8S" });

  it("puts a project's general, team and per-agent log channels under that project", () => {
    const guilds = buildGuilds([ada, bruno, zoe], [k8s], []);
    const project = guilds.find((g) => g.id === "k8s")!;
    expect(project.kind).toBe("project");
    expect(project.channels.map((c) => c.channelId)).toEqual([
      "ws:k8s:general",
      "team:ada",
      "ws:k8s:log:ada",
      "ws:k8s:log:bruno",
    ]);
    expect(project.channels.filter((c) => c.kind === "log").map((c) => c.slug)).toEqual([
      "log-ada",
      "log-bruno",
    ]);
  });

  it("keeps the company room, and company-wide teams, outside every project", () => {
    const quill = persona({ id: "quill", name: "Quill", reports_to: "zoe" });
    const guilds = buildGuilds([ada, bruno, zoe, quill], [k8s], []);
    expect(guilds.map((g) => g.id)).toEqual([COMPANY, "k8s"]);
    const company = guilds[0];
    expect(company.kind).toBe("company");
    // Zoe leads nobody on a project, so her room stays at company level.
    expect(company.channels.map((c) => c.channelId)).toEqual(["company", "team:zoe"]);
  });

  it("leaves direct messages out of the guilds entirely", () => {
    const guilds = buildGuilds([ada, bruno, zoe], [k8s], []);
    expect(allGuildChannels(guilds).some((c) => c.kind === "dm")).toBe(false);
    expect(dmChannels([ada, zoe]).map((c) => c.channelId)).toEqual(["dm:ada", "dm:zoe"]);
  });

  it("gives a project with nobody on it a general room and no logs", () => {
    const guilds = buildGuilds([zoe], [ws({ id: "shop", name: "Shop" })], []);
    const shop = guilds.find((g) => g.id === "shop")!;
    expect(shop.channels.map((c) => c.kind)).toEqual(["project"]);
  });
});

describe("messageInChannel with channel ids", () => {
  const ada = persona({ id: "ada", name: "Ada", workspace_id: "k8s" });
  const bruno = persona({ id: "bruno", name: "Bruno", reports_to: "ada", workspace_id: "k8s" });
  const guilds = buildGuilds([ada, bruno], [ws({ id: "k8s", name: "K8S" })], []);
  const general = allGuildChannels(guilds).find((c) => c.channelId === "ws:k8s:general")!;
  const logAda = allGuildChannels(guilds).find((c) => c.channelId === "ws:k8s:log:ada")!;
  const logBruno = allGuildChannels(guilds).find((c) => c.channelId === "ws:k8s:log:bruno")!;
  const company = allGuildChannels(guilds).find((c) => c.channelId === "company")!;

  it("routes a stamped message by its channel id and nowhere else", () => {
    const m = msg({ id: "1", body: "restarting", channel_id: "ws:k8s:log:ada", workspace_id: "k8s" });
    expect(messageInChannel(m, logAda)).toBe(true);
    // The pair the old derivation could not tell apart: same project, same people.
    expect(messageInChannel(m, general)).toBe(false);
    expect(messageInChannel(m, logBruno)).toBe(false);
    expect(messageInChannel(m, company)).toBe(false);
  });

  it("still routes a legacy row with no channel id exactly as it did before", () => {
    const legacy = msg({ id: "2", body: "deploy", from_id: "ada", workspace_id: "k8s" });
    expect(messageInChannel(legacy, general)).toBe(true);
    expect(messageInChannel(legacy, company)).toBe(false);
    const unscoped = msg({ id: "3", body: "hello", from_id: "ada" });
    expect(messageInChannel(unscoped, company)).toBe(true);
    expect(messageInChannel(unscoped, general)).toBe(false);
  });

  it("never lets a legacy row leak into a log channel it cannot possibly belong to", () => {
    const legacy = msg({ id: "4", body: "deploy", from_id: "ada", workspace_id: "k8s" });
    expect(messageInChannel(legacy, logAda)).toBe(false);
  });
});

describe("parseTs", () => {
  it("reads SQLite's zoneless UTC as UTC, not as local time", () => {
    expect(parseTs("2026-09-01 16:00:00")).toBe(Date.parse("2026-09-01T16:00:00Z"));
    expect(parseTs("2026-09-01T16:00:00Z")).toBe(Date.parse("2026-09-01T16:00:00Z"));
  });

  it("is NaN for nothing and for nonsense, so a caller can decide what that means", () => {
    expect(Number.isNaN(parseTs(null))).toBe(true);
    expect(Number.isNaN(parseTs("not a date"))).toBe(true);
  });
});
