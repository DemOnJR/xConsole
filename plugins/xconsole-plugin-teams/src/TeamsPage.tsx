import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  api,
  onAgentMessage,
  type AgentMessage,
  type GoalSession,
  type Persona,
  type Workspace,
} from "../../../src/lib/tauri";
import { CloseIcon, HashIcon, SendIcon, UsersIcon } from "../../../src/components/icons";
import { usePersonaStatusStore } from "../../../src/stores/personaStatusStore";
import { memberLive, phaseColor } from "./status";
import {
  avatarColor,
  buildChannels,
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

function clock(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function TeamsPage({ onClose }: { onClose?: () => void }) {
  const [personas, setPersonas] = useState<Persona[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [goals, setGoals] = useState<GoalSession[]>([]);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [channelId, setChannelId] = useState<string>(COMPANY);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = usePersonaStatusStore((s) => s.byKey);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

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

  useEffect(() => {
    let alive = true;
    api
      .listAgentMessages(null, null, 500)
      .then((m) => {
        if (alive) setMessages(m);
      })
      .catch(() => {
        if (alive) setMessages([]);
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onAgentMessage((msg) => {
      setMessages((prev) => (prev.some((m) => m.id === msg.id) ? prev : [...prev, msg]));
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const channels = useMemo(
    () => buildChannels(personas, workspaces, goals),
    [personas, workspaces, goals],
  );
  const dms = useMemo(() => dmChannels(personas), [personas]);
  const allChannels = useMemo(() => [...channels, ...dms], [channels, dms]);
  const channel = allChannels.find((c) => c.id === channelId) || channels[0];

  useEffect(() => {
    if (channelId !== COMPANY && !allChannels.some((c) => c.id === channelId) && channels[0]) {
      setChannelId(channels[0].id);
    }
  }, [allChannels, channelId, channels]);

  const visible = useMemo(
    () => (channel ? messages.filter((m) => messageInChannel(m, channel)) : []),
    [messages, channel],
  );
  const groups = useMemo(() => groupMessages(visible), [visible]);

  useEffect(() => {
    stickToBottom.current = true;
  }, [channelId]);

  useEffect(() => {
    const el = scroller.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [visible.length, channelId, groups.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const members = useMemo(() => {
    if (!channel) return [];
    const byId = new Map(personas.map((p) => [p.id, p]));
    return channel.memberIds.map((id) => byId.get(id)).filter((p): p is Persona => Boolean(p));
  }, [channel, personas]);

  const send = async () => {
    const body = draft.trim();
    if (!body || sending || !channel) return;
    setSending(true);
    setError(null);
    try {
      const to =
        channel.kind === "dm"
          ? channel.memberIds[0]
          : channel.kind === "team"
            ? channel.leadId
            : null;
      const ws =
        channel.kind === "project"
          ? channel.workspaceId
          : channel.kind === "dm"
            ? channel.workspaceId
            : channel.kind === "team"
              ? channel.workspaceId
              : null;
      await api.postAgentMessage(body, to, ws, to ? "request" : "note");
      setDraft("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSending(false);
    }
  };

  const headerTitle = channel
    ? channel.kind === "dm"
      ? channel.title
      : `#${channel.slug}`
    : "Teams";
  const placeholder = channel
    ? channel.kind === "dm"
      ? `Message ${channel.title}…`
      : `Message #${channel.slug}`
    : "Message…";

  return (
    <div className="flex h-full min-h-0 flex-col bg-[var(--bg)] text-[var(--text)]">
      <header className="flex items-center gap-3 border-b border-[var(--border)] px-4 py-3">
        <UsersIcon size={18} className="text-[var(--text-faint)]" />
        <div className="min-w-0 flex-1">
          <h1 className="text-[13px] font-semibold tracking-wide">Teams</h1>
          <p className="truncate text-[11px] text-[var(--text-faint)]">
            Channels per project and team, and what they actually say
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
        <aside className="flex w-[220px] shrink-0 flex-col overflow-y-auto border-r border-[var(--border)] bg-[var(--surface-2)]">
          <ChannelSection label="Channels">
            {channels.map((c) => (
              <ChannelRow
                key={c.id}
                channel={c}
                active={channelId === c.id}
                onSelect={() => setChannelId(c.id)}
              />
            ))}
          </ChannelSection>
          <ChannelSection label="Direct messages">
            {dms.length === 0 ? (
              <p className="px-3 py-4 text-[11px] text-[var(--text-faint)]">
                No agents yet. Hire them in Settings → Agents.
              </p>
            ) : (
              dms.map((c) => {
                const p = personas.find((x) => x.id === c.memberIds[0]);
                const st = p ? memberLive(p, live, goals) : null;
                return (
                  <ChannelRow
                    key={c.id}
                    channel={c}
                    active={channelId === c.id}
                    onSelect={() => setChannelId(c.id)}
                    status={st?.phase}
                    enabled={p?.enabled !== false}
                  />
                );
              })
            )}
          </ChannelSection>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-2 border-b border-[var(--border)] px-4 py-2">
            {channel?.kind === "dm" ? (
              <span
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold text-[var(--bg)]"
                style={{ background: avatarColor(channel.memberIds[0] || channel.title) }}
              >
                {initials(channel.title)}
              </span>
            ) : (
              <HashIcon size={14} className="text-[var(--text-faint)]" />
            )}
            <h2 className="text-[13px] font-semibold">{headerTitle}</h2>
            {channel && (
              <span className="min-w-0 truncate text-[11px] text-[var(--text-faint)]">
                {channel.subtitle}
                {channel.kind !== "dm" ? ` · ${members.length} people` : ""}
              </span>
            )}
          </div>

          <div
            ref={scroller}
            className="min-h-0 flex-1 overflow-y-auto px-4 py-3"
            onScroll={(e) => {
              const el = e.currentTarget;
              stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
            }}
          >
            {groups.length === 0 ? (
              <p className="py-10 text-center text-[11px] text-[var(--text-faint)]">
                {channel?.kind === "project"
                  ? `Nothing said in #${channel.slug} yet. Messages about this project land here.`
                  : channel?.kind === "team"
                    ? `Nothing in #${channel.slug} yet. What this team says to each other appears here.`
                    : "Nothing said yet on this thread."}
              </p>
            ) : (
              groups.map((g, i) => {
                const prev = groups[i - 1];
                const showDay = dayKey(g.messages[0].created_at) !== dayKey(prev?.messages[0].created_at);
                const fromName = nameOf(personas, g.fromId);
                const toName = g.toId ? nameOf(personas, g.toId) : null;
                const color = avatarColor(g.fromId || "you");
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
                        style={{ background: color }}
                      >
                        {initials(fromName)}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                          <span className="text-[13px] font-semibold text-[var(--text)]">{fromName}</span>
                          {toName && toName !== fromName && (
                            <span className="text-[11px] text-[var(--text-faint)]">to {toName}</span>
                          )}
                          <KindPill kind={g.kind} />
                          <span className="font-mono text-[10px] text-[var(--text-faint)]">
                            {clock(g.messages[0].created_at)}
                          </span>
                        </div>
                        {g.messages.map((m) => (
                          <p
                            key={m.id}
                            className="whitespace-pre-wrap text-[13px] leading-relaxed text-[var(--text)]"
                          >
                            {m.body}
                          </p>
                        ))}
                      </div>
                    </div>
                  </div>
                );
              })
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

        <aside className="flex w-[220px] shrink-0 flex-col border-l border-[var(--border)]">
          <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
            {channel?.kind === "dm" ? "Profile" : "Members"}
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
                    onClick={() => setChannelId(`dm:${p.id}`)}
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
                        {!p.enabled && (
                          <span className="text-[10px] text-[var(--text-faint)]">off</span>
                        )}
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
      </div>
    </div>
  );
}

function ChannelSection({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <div className="px-3 pt-3 pb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
        {label}
      </div>
      <div className="px-1 pb-2">{children}</div>
    </div>
  );
}

function ChannelRow({
  channel,
  active,
  onSelect,
  status,
  enabled = true,
}: {
  channel: Channel;
  active: boolean;
  onSelect: () => void;
  status?: string;
  enabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`mb-0.5 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-[12px] ${
        active
          ? "bg-[var(--accent-muted)] text-[var(--text)]"
          : "text-[var(--text-dim)] hover:bg-[var(--border)]/40 hover:text-[var(--text)]"
      } ${enabled ? "" : "opacity-50"}`}
    >
      {channel.kind === "dm" ? (
        <span className="relative flex h-4 w-4 shrink-0 items-center justify-center">
          <span
            className="flex h-4 w-4 items-center justify-center rounded-full text-[8px] font-semibold text-[var(--bg)]"
            style={{ background: avatarColor(channel.memberIds[0] || channel.title) }}
          >
            {initials(channel.title)}
          </span>
          {status && (
            <span
              className="absolute -bottom-0.5 -right-0.5 h-1.5 w-1.5 rounded-full border border-[var(--surface-2)]"
              style={{ background: phaseColor(status) }}
            />
          )}
        </span>
      ) : (
        <HashIcon size={12} className="shrink-0 text-[var(--text-faint)]" />
      )}
      <span className="min-w-0 flex-1 truncate">
        {channel.kind === "dm" ? channel.title : channel.slug}
      </span>
    </button>
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
