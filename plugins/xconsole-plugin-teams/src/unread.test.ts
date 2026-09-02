import { describe, expect, it } from "vitest";
import type { AgentMessage, ChannelUnread, Persona, Workspace } from "../../../src/lib/tauri";
import { buildGuilds, COMPANY } from "./channels";
import {
  badge,
  cursorsFrom,
  mentionIds,
  unreadByChannel,
  unreadForGuild,
  unreadInChannel,
} from "./unread";

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

const msg = (partial: Partial<AgentMessage> & Pick<AgentMessage, "id" | "body">): AgentMessage => ({
  kind: "note",
  ...partial,
});

const CURSOR = "2026-09-01 16:00:00";
const before = msg({
  id: "old",
  body: "old",
  from_id: "ada",
  channel_id: "company",
  created_at: "2026-09-01 15:00:00",
});
const after = msg({
  id: "new",
  body: "new",
  from_id: "ada",
  channel_id: "company",
  created_at: "2026-09-01 17:00:00",
});

describe("unreadInChannel", () => {
  it("counts only what arrived after the cursor", () => {
    expect(unreadInChannel([before, after], "company", CURSOR, null)).toEqual({
      count: 1,
      mentions: 0,
    });
  });

  it("counts everything when the reader has never opened the room", () => {
    expect(unreadInChannel([before, after], "company", null, null).count).toBe(2);
    expect(unreadInChannel([before, after], "company", undefined, null).count).toBe(2);
  });

  it("never counts your own messages, whether you are the user or an agent", () => {
    const mine = msg({ id: "mine", body: "mine", channel_id: "company" });
    // from_id null is the user.
    expect(unreadInChannel([mine], "company", null, null).count).toBe(0);
    expect(unreadInChannel([mine], "company", null, "ada").count).toBe(1);
    const adas = msg({ id: "a", body: "a", from_id: "ada", channel_id: "company" });
    expect(unreadInChannel([adas], "company", null, "ada").count).toBe(0);
  });

  it("badges a mention separately from the plain count", () => {
    const named = msg({
      id: "n",
      body: "@Ada have a look",
      from_id: "bruno",
      channel_id: "company",
      mentions: ["ada"],
    });
    expect(unreadInChannel([named], "company", null, "ada")).toEqual({ count: 1, mentions: 1 });
    // The user is not a persona, so nothing ever mentions them.
    expect(unreadInChannel([named], "company", null, null)).toEqual({ count: 1, mentions: 0 });
    expect(unreadInChannel([named], "company", null, "bruno").count).toBe(0);
  });

  it("ignores rows belonging to another room, and legacy rows with no room at all", () => {
    const elsewhere = msg({ id: "e", body: "e", from_id: "ada", channel_id: "ws:k8s:general" });
    const legacy = msg({ id: "l", body: "l", from_id: "ada" });
    expect(unreadInChannel([elsewhere, legacy], "company", null, null).count).toBe(0);
  });

  it("counts a message whose timestamp will not parse rather than swallowing it", () => {
    const broken = msg({ id: "b", body: "b", from_id: "ada", channel_id: "company", created_at: "" });
    expect(unreadInChannel([broken], "company", CURSOR, null).count).toBe(1);
  });
});

describe("unreadByChannel", () => {
  it("keys the counts by room, each against its own cursor", () => {
    const rows = [
      before,
      after,
      msg({
        id: "k",
        body: "k",
        from_id: "ada",
        channel_id: "ws:k8s:general",
        created_at: "2026-09-01 17:00:00",
      }),
    ];
    const got = unreadByChannel(rows, { company: CURSOR }, null);
    expect(got.company).toEqual({ count: 1, mentions: 0 });
    expect(got["ws:k8s:general"]).toEqual({ count: 1, mentions: 0 });
  });

  it("leaves a fully read room out of the map entirely", () => {
    expect(unreadByChannel([before], { company: CURSOR }, null)).toEqual({});
  });
});

describe("unreadForGuild", () => {
  it("sums every room behind one rail tile", () => {
    const ada = persona({ id: "ada", name: "Ada", workspace_id: "k8s" });
    const guilds = buildGuilds([ada], [ws({ id: "k8s", name: "K8S" })], []);
    const byChannel = {
      "ws:k8s:general": { count: 2, mentions: 1 },
      "ws:k8s:log:ada": { count: 3, mentions: 0 },
      company: { count: 9, mentions: 9 },
    };
    const project = guilds.find((g) => g.id === "k8s")!;
    expect(unreadForGuild(project, byChannel)).toEqual({ count: 5, mentions: 1 });
    const company = guilds.find((g) => g.id === COMPANY)!;
    expect(unreadForGuild(company, byChannel)).toEqual({ count: 9, mentions: 9 });
  });
});

describe("cursorsFrom", () => {
  it("keeps a null cursor as null, meaning never opened", () => {
    const rows: ChannelUnread[] = [
      { channel_id: "company", unread: 2, mentions: 0, last_read_at: "2026-09-01 16:00:00" },
      { channel_id: "dm:ada", unread: 1, mentions: 0, last_read_at: null },
    ];
    expect(cursorsFrom(rows)).toEqual({
      company: "2026-09-01 16:00:00",
      "dm:ada": null,
    });
  });
});

describe("mentionIds", () => {
  const staff = [
    persona({ id: "ada", name: "Ada" }),
    persona({ id: "ada-l", name: "Ada Lovelace" }),
    persona({ id: "bruno", name: "Bruno" }),
  ];

  it("resolves a name against the staff list, not against a pattern", () => {
    expect(mentionIds("@Bruno can you look", staff)).toEqual(["bruno"]);
    expect(mentionIds("sha is @deadbeef", staff)).toEqual([]);
  });

  it("prefers the longer name so a full name is not eaten by a prefix", () => {
    expect(mentionIds("@Ada Lovelace please", staff)).toEqual(["ada-l"]);
  });

  it("is case-insensitive and never repeats an id", () => {
    expect(mentionIds("@ada and @Ada again", staff)).toEqual(["ada"]);
  });
});

describe("badge", () => {
  it("shows nothing at zero and stops being exact past ninety-nine", () => {
    expect(badge(0)).toBe("");
    expect(badge(-1)).toBe("");
    expect(badge(7)).toBe("7");
    expect(badge(150)).toBe("99+");
  });
});
