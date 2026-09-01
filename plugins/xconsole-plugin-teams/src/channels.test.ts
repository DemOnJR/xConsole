import { describe, expect, it } from "vitest";
import type { AgentMessage, GoalSession, Persona, Workspace } from "../../../src/lib/tauri";
import {
  avatarColor,
  buildChannels,
  dmChannels,
  groupMessages,
  initials,
  membersForProject,
  messageInChannel,
  nameOf,
  reportsTree,
  slugify,
  vpsIdsOfWorkspace,
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
