import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  onAgentLog,
  onAgentMessage,
  onPersonaStatus,
  type AgentLogEntry,
  type AgentMessage,
  type GoalSession,
  type Persona,
  type Workspace,
} from "../../../src/lib/tauri";
import {
  CloseIcon,
  ConversationIcon,
  HashIcon,
  SendIcon,
  TerminalIcon,
  UsersIcon,
} from "../../../src/components/icons";
import {
  usePersonaStatusStore,
  type PersonaStatusEntry,
} from "../../../src/stores/personaStatusStore";
import { memberLive, phaseColor } from "./status";
import {
  allGuildChannels,
  avatarColor,
  buildGuilds,
  COMPANY,
  dayKey,
  dayLabel,
  dmChannels,
  groupMessages,
  initials,
  messageInChannel,
  nameOf,
  type Channel,
} from "./channels";
import { mergeLog, type LiveLogLine, type LogLine } from "./log";
import { buildIndex, replyCounts, repliesTo, resolveParent, threadRoots } from "./threads";
import { cursorsFrom, mentionIds, unreadByChannel, unreadForGuild, type ReadCursors } from "./unread";
import { GuildRail } from "./GuildRail";
import { ChannelList } from "./ChannelList";
import { LogFeed } from "./LogFeed";
import { ThreadPanel } from "./ThreadPanel";

/** How much of each agent's live status tail is worth keeping in memory. */
const LIVE_TAIL = 200;

function clock(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date((iso || "").replace(" ", "T"));
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function TeamsPage({ onClose }: { onClose?: () => void }) {
  const [personas, setPersonas] = useState<Persona[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [goals, setGoals] = useState<GoalSession[]>([]);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [logs, setLogs] = useState<Record<string, AgentLogEntry[]>>({});
  const [tails, setTails] = useState<Record<string, LiveLogLine[]>>({});
  const [cursors, setCursors] = useState<ReadCursors>({});
  const [guildId, setGuildId] = useState<string>(COMPANY);
  const [channelId, setChannelId] = useState<string>(COMPANY);
  const [threadParentId, setThreadParentId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = usePersonaStatusStore((s) => s.byKey);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  /** The open room, for the live listener, which is registered once and never re-run. */
  const openChannel = useRef<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const [p, w, g] = await Promise.all([
        api.listPersonas(),
        api.listWorkspaces(),
        api.listGoals(),
      ]);
      setPersonas(p);
      setWorkspaces(w);
      setGoals(g);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void reload();
    const t = window.setInterval(() => void reload(), 8000);
    return () => window.clearInterval(t);
  }, [reload]);

  const absorb = useCallback((incoming: AgentMessage[]) => {
    setMessages((prev) => {
      const seen = new Set(prev.map((m) => m.id));
      const add = incoming.filter((m) => !seen.has(m.id));
      return add.length ? [...prev, ...add] : prev;
    });
  }, []);

  useEffect(() => {
    let alive = true;
    // One bulk read covers every room at once; a per-room fetch on open tops up the
    // history of whichever room is being read.
    api
      .listAgentMessages(null, null, 500)
      .then((m) => alive && setMessages(m))
      .catch(() => alive && setMessages([]));
    api
      .channelUnread()
      .then((rows) => alive && setCursors(cursorsFrom(rows)))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onAgentMessage((msg) => {
      absorb([msg]);
      // Arriving in the room you are looking at is not unread. Told to the backend as
      // well as held locally, so the badge does not come back on the next reload.
      if (msg.channel_id && msg.channel_id === openChannel.current) {
        setCursors((c) => ({ ...c, [msg.channel_id as string]: new Date().toISOString() }));
        void api.markChannelRead(msg.channel_id).catch(() => {});
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [absorb]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onAgentLog((entry) => {
      setLogs((prev) => {
        const at = prev[entry.persona_id] ?? [];
        if (at.some((e) => e.id === entry.id)) return prev;
        return { ...prev, [entry.persona_id]: [...at, entry].slice(-LIVE_TAIL * 2) };
      });
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    // The status store keeps only the latest phase per agent, which answers "what is Ada
    // doing" and not "what has Ada been doing". A log channel needs the tail, so keep one.
    onPersonaStatus((s) => {
      if (!s.persona_id || s.status === "idle") return;
      const line: LiveLogLine = {
        personaId: s.persona_id,
        status: s.status,
        tool: null,
        detail: s.detail || "",
        at: Date.now(),
      };
      setTails((prev) => {
        const at = prev[line.personaId] ?? [];
        return { ...prev, [line.personaId]: [...at, line].slice(-LIVE_TAIL) };
      });
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const guilds = useMemo(
    () => buildGuilds(personas, workspaces, goals),
    [personas, workspaces, goals],
  );
  const dms = useMemo(() => dmChannels(personas), [personas]);
  const allChannels = useMemo(() => [...allGuildChannels(guilds), ...dms], [guilds, dms]);
  const guild = guilds.find((g) => g.id === guildId) ?? guilds[0];
  const channel: Channel | undefined =
    allChannels.find((c) => c.id === channelId) ?? guild?.channels[0];

  // A project deleted under you, or an agent dismissed, leaves a selection pointing at
  // nothing. Fall back rather than render an empty shell.
  useEffect(() => {
    if (!allChannels.length) return;
    if (!allChannels.some((c) => c.id === channelId)) {
      const fallback = guilds.find((g) => g.id === guildId)?.channels[0] ?? allChannels[0];
      if (fallback) setChannelId(fallback.id);
    }
  }, [allChannels, channelId, guildId, guilds]);

  const selectGuild = (id: string) => {
    setGuildId(id);
    const first = guilds.find((g) => g.id === id)?.channels[0];
    if (first) setChannelId(first.id);
    setThreadParentId(null);
  };

  const selectChannel = (c: Channel) => {
    setChannelId(c.id);
    setThreadParentId(null);
  };

  // Opening a room: mark it read, and top up its history past the bulk window.
  useEffect(() => {
    if (!channel) return;
    const id = channel.channelId;
    openChannel.current = id;
    stickToBottom.current = true;
    setCursors((c) => ({ ...c, [id]: new Date().toISOString() }));
    void api.markChannelRead(id).catch(() => {});
    let alive = true;
    api
      .listChannelMessages(id, 300)
      .then((rows) => alive && absorb(rows))
      .catch(() => {});
    if (channel.kind === "log" && channel.personaId) {
      const pid = channel.personaId;
      api
        .listAgentLog(pid, 300)
        .then((rows) => alive && setLogs((prev) => ({ ...prev, [pid]: rows })))
        .catch(() => {});
    }
    return () => {
      alive = false;
    };
  }, [channel?.channelId, channel?.kind, channel?.personaId, absorb]);

  const inChannel = useMemo(
    () => (channel ? messages.filter((m) => messageInChannel(m, channel)) : []),
    [messages, channel],
  );
  const logLines: LogLine[] = useMemo(() => {
    if (channel?.kind !== "log" || !channel.personaId) return [];
    return mergeLog(logs[channel.personaId] ?? [], tails[channel.personaId] ?? []);
  }, [channel, logs, tails]);

  const index = useMemo(
    () => buildIndex(messages, Object.values(logs).flat()),
    [messages, logs],
  );
  const roots = useMemo(() => threadRoots(inChannel, index), [inChannel, index]);
  const counts = useMemo(() => replyCounts(messages), [messages]);
  const groups = useMemo(() => groupMessages(roots), [roots]);

  const unreadPerChannel = useMemo(
    () => unreadByChannel(messages, cursors, null),
    [messages, cursors],
  );
  const unreadPerGuild = useMemo(() => {
    const out: Record<string, { count: number; mentions: number }> = {};
    for (const g of guilds) out[g.id] = unreadForGuild(g, unreadPerChannel);
    return out;
  }, [guilds, unreadPerChannel]);

  const thread = useMemo(() => {
    if (!threadParentId) return null;
    const fromLog = logLines.find((l) => l.id === threadParentId);
    const resolved = resolveParent(threadParentId, index);
    return {
      id: threadParentId,
      parent:
        resolved ??
        (fromLog
          ? ({
              kind: "log" as const,
              id: fromLog.id,
              entry: {
                id: fromLog.id,
                persona_id: fromLog.personaId,
                session_id: "",
                status: fromLog.status,
                tool: fromLog.tool,
                detail: fromLog.detail,
                created_at: new Date(fromLog.at).toISOString(),
              } satisfies AgentLogEntry,
            })
          : null),
      replies: repliesTo(threadParentId, messages),
    };
  }, [threadParentId, index, logLines, messages]);

  useEffect(() => {
    const el = scroller.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [groups.length, logLines.length, channelId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (threadParentId) {
        e.preventDefault();
        setThreadParentId(null);
        return;
      }
      if (onClose) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, threadParentId]);

  const members = useMemo(() => {
    if (!channel) return [];
    const byId = new Map(personas.map((p) => [p.id, p]));
    return channel.memberIds.map((id) => byId.get(id)).filter((p): p is Persona => Boolean(p));
  }, [channel, personas]);

  const post = useCallback(
    async (body: string, parentId: string | null) => {
      if (!channel) return;
      const sent = await api.postChannelMessage(
        channel.channelId,
        body,
        parentId,
        mentionIds(body, personas),
      );
      absorb([sent]);
    },
    [channel, personas, absorb],
  );

  const send = async () => {
    const body = draft.trim();
    if (!body || sending || !channel) return;
    setSending(true);
    setError(null);
    try {
      await post(body, null);
      setDraft("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSending(false);
    }
  };

  const isLog = channel?.kind === "log";
  const headerTitle = channel
    ? channel.kind === "dm"
      ? channel.title
      : `#${channel.slug}`
    : "Teams";
  const placeholder = channel
    ? channel.kind === "dm"
      ? `Message ${channel.title}`
      : isLog
        ? `Add a note to #${channel.slug}`
        : `Message #${channel.slug}`
    : "Message";

  return (
    <div className="flex h-full min-h-0 flex-col bg-[var(--bg)] text-[var(--text)]">
      <header className="flex items-center gap-3 border-b border-[var(--border)] px-4 py-3">
        <UsersIcon size={18} className="text-[var(--text-faint)]" />
        <div className="min-w-0 flex-1">
          <h1 className="text-[13px] font-semibold tracking-wide">Teams</h1>
          <p className="truncate text-[11px] text-[var(--text-faint)]">
            A channel per project and team, a live log per agent, and a thread on anything
          </p>
        </div>
        {onClose && (
          <button type="button" className="xc-icon-btn" data-tooltip="Close" onClick={onClose}>
            <CloseIcon size={16} />
          </button>
        )}
      </header>

      {error && (
        <div className="border-b border-red-900/40 bg-red-950/40 px-4 py-2 text-[11px] text-red-300">
          {error}
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <GuildRail
          guilds={guilds}
          activeId={guild?.id ?? COMPANY}
          unread={unreadPerGuild}
          onSelect={selectGuild}
        />
        <ChannelList
          guild={guild}
          dms={dms}
          personas={personas}
          goals={goals}
          live={live}
          activeId={channel?.id ?? ""}
          unread={unreadPerChannel}
          onSelect={selectChannel}
        />

        <section className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-2 border-b border-[var(--border)] px-4 py-2">
            {channel?.kind === "dm" ? (
              <span
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold text-[var(--bg)]"
                style={{ background: avatarColor(channel.personaId || channel.title) }}
              >
                {initials(channel.title)}
              </span>
            ) : isLog ? (
              <TerminalIcon size={14} className="text-[var(--text-faint)]" />
            ) : (
              <HashIcon size={14} className="text-[var(--text-faint)]" />
            )}
            <h2 className="text-[13px] font-semibold">{headerTitle}</h2>
            {channel && (
              <span className="min-w-0 truncate text-[11px] text-[var(--text-faint)]">
                {channel.subtitle}
                {channel.kind !== "dm" && !isLog ? ` · ${members.length} people` : ""}
              </span>
            )}
          </div>

          <div
            ref={scroller}
            className="flex min-h-0 flex-1 flex-col overflow-y-auto px-2 py-3"
            onScroll={(e) => {
              const el = e.currentTarget;
              stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
            }}
          >
            {isLog && (
              <LogFeed
                lines={logLines}
                name={nameOf(personas, channel?.personaId)}
                activeParentId={threadParentId}
                onReply={(line) => setThreadParentId(line.id)}
                replyCounts={counts}
              />
            )}

            {groups.length === 0 && !isLog ? (
              <p className="px-2 py-10 text-center text-[11px] text-[var(--text-faint)]">
                {channel?.kind === "project"
                  ? `Nothing said in #${channel.slug} yet. Anything you post here reaches this project's agents.`
                  : channel?.kind === "team"
                    ? `Nothing in #${channel.slug} yet. What this team says to each other appears here.`
                    : channel?.kind === "dm"
                      ? "Nothing said yet on this thread."
                      : "Nothing said company-wide yet. Name someone with @ to reach them."}
              </p>
            ) : (
              <div className="px-2">
                {isLog && groups.length > 0 && (
                  <div className="my-3 flex items-center gap-3">
                    <div className="h-px flex-1 bg-[var(--border)]" />
                    <span className="text-[10px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                      Notes on this log
                    </span>
                    <div className="h-px flex-1 bg-[var(--border)]" />
                  </div>
                )}
                {groups.map((g, i) => {
                  const prev = groups[i - 1];
                  const showDay =
                    dayKey(g.messages[0].created_at) !== dayKey(prev?.messages[0].created_at);
                  const fromName = nameOf(personas, g.fromId);
                  const toName = g.toId ? nameOf(personas, g.toId) : null;
                  return (
                    <div key={g.key}>
                      {showDay && (
                        <div className="my-3 flex items-center gap-3">
                          <div className="h-px flex-1 bg-[var(--border)]" />
                          <span className="text-[10px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                            {dayLabel(g.messages[0].created_at)}
                          </span>
                          <div className="h-px flex-1 bg-[var(--border)]" />
                        </div>
                      )}
                      <div className="mb-3 flex gap-2.5">
                        <span
                          className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-[11px] font-semibold text-[var(--bg)]"
                          style={{ background: avatarColor(g.fromId || "you") }}
                        >
                          {initials(fromName)}
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                            <span className="text-[13px] font-semibold text-[var(--text)]">
                              {fromName}
                            </span>
                            {toName && toName !== fromName && (
                              <span className="text-[11px] text-[var(--text-faint)]">
                                to {toName}
                              </span>
                            )}
                            <KindPill kind={g.kind} />
                            <span className="font-mono text-[10px] text-[var(--text-faint)]">
                              {clock(g.messages[0].created_at)}
                            </span>
                          </div>
                          {g.messages.map((m) => (
                            <div key={m.id} className="group flex items-start gap-2">
                              <p className="min-w-0 flex-1 whitespace-pre-wrap break-words text-[13px] leading-relaxed text-[var(--text)]">
                                {m.body}
                              </p>
                              <button
                                type="button"
                                onClick={() => setThreadParentId(m.id)}
                                data-tooltip="Reply in thread"
                                className={`mt-0.5 shrink-0 rounded p-0.5 text-[var(--text-faint)] hover:text-[var(--text)] ${
                                  (counts.get(m.id) ?? 0) > 0
                                    ? ""
                                    : "opacity-0 group-hover:opacity-100"
                                }`}
                              >
                                <ConversationIcon size={12} />
                              </button>
                              {(counts.get(m.id) ?? 0) > 0 && (
                                <span className="mt-0.5 shrink-0 rounded-sm bg-[var(--accent-muted)] px-1 font-mono text-[10px] text-[var(--text-dim)]">
                                  {counts.get(m.id)}
                                </span>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <form
            className="flex items-end gap-2 border-t border-[var(--border)] px-3 py-2"
            onSubmit={(e) => {
              e.preventDefault();
              void send();
            }}
          >
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  void send();
                }
              }}
              rows={1}
              placeholder={placeholder}
              className="max-h-32 min-h-[34px] min-w-0 flex-1 resize-none rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5 text-[13px] outline-none focus:border-[var(--accent)]"
            />
            <button
              type="submit"
              disabled={sending || !draft.trim()}
              className="xc-icon-btn mb-0.5"
              data-tooltip="Send"
            >
              <SendIcon size={14} />
            </button>
          </form>
        </section>

        {thread ? (
          <ThreadPanel
            parent={thread.parent}
            parentId={thread.id}
            replies={thread.replies}
            personas={personas}
            onClose={() => setThreadParentId(null)}
            onSend={async (body) => {
              try {
                await post(body, thread.id);
              } catch (e) {
                setError(String(e));
              }
            }}
          />
        ) : (
          <MembersPanel
            channel={channel}
            members={members}
            goals={goals}
            live={live}
            onOpenDm={(id) => {
              setGuildId(COMPANY);
              setChannelId(`dm:${id}`);
              setThreadParentId(null);
            }}
          />
        )}
      </div>
    </div>
  );
}

function MembersPanel({
  channel,
  members,
  goals,
  live,
  onOpenDm,
}: {
  channel: Channel | undefined;
  members: Persona[];
  goals: GoalSession[];
  live: Record<string, PersonaStatusEntry>;
  onOpenDm: (personaId: string) => void;
}) {
  return (
    <aside className="flex w-[210px] shrink-0 flex-col border-l border-[var(--border)]">
      <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
        {channel?.kind === "dm" ? "Profile" : channel?.kind === "log" ? "Agent" : "Members"}
      </div>
      <div className="flex-1 overflow-y-auto px-1 pb-2">
        {members.length === 0 ? (
          <p className="px-2 py-6 text-center text-[11px] text-[var(--text-faint)]">
            No agents on this channel.
          </p>
        ) : (
          members.map((p) => {
            const st = memberLive(p, live, goals);
            return (
              <button
                key={p.id}
                type="button"
                onClick={() => onOpenDm(p.id)}
                className="mb-0.5 flex w-full items-start gap-2 rounded px-2 py-1.5 text-left hover:bg-[var(--border)]/40"
              >
                <span className="relative mt-0.5 shrink-0">
                  <span
                    className="flex h-7 w-7 items-center justify-center rounded-full text-[10px] font-semibold text-[var(--bg)]"
                    style={{ background: avatarColor(p.id) }}
                  >
                    {initials(p.name)}
                  </span>
                  <span
                    className="absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full border border-[var(--bg)]"
                    style={{ background: phaseColor(st.phase) }}
                  />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1">
                    <span className="truncate text-[12px] text-[var(--text)]">{p.name}</span>
                    {!p.enabled && <span className="text-[10px] text-[var(--text-faint)]">off</span>}
                  </span>
                  <span className="block truncate font-mono text-[10px] text-[var(--text-faint)]">
                    {st.label}
                  </span>
                  {st.task && (
                    <span className="block truncate text-[10px] text-[var(--text-faint)]">
                      {st.task}
                    </span>
                  )}
                </span>
              </button>
            );
          })
        )}
      </div>
    </aside>
  );
}

function KindPill({ kind }: { kind: string }) {
  if (kind === "note") return null;
  return (
    <span
      className={`rounded px-1 py-px text-[9px] font-medium uppercase tracking-wide ${
        kind === "report"
          ? "bg-[var(--accent-muted)] text-[var(--text-dim)]"
          : "text-[var(--text-faint)]"
      }`}
    >
      {kind}
    </span>
  );
}
