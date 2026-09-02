import type { ReactNode } from "react";
import type { GoalSession, Persona } from "../../../src/lib/tauri";
import { HashIcon, TerminalIcon } from "../../../src/components/icons";
import type { PersonaStatusEntry } from "../../../src/stores/personaStatusStore";
import { avatarColor, initials, type Channel, type Guild } from "./channels";
import { memberLive, phaseColor } from "./status";
import { badge, type UnreadCount } from "./unread";

/**
 * The rooms behind one rail tile.
 *
 * Live logs are their own section rather than mixed in with the talking channels: they
 * are read for a different reason (what is happening right now) and at a different
 * pace, and a `#log-ada` sorted alphabetically between `#general` and `#team` reads as
 * just another chat room, which it is not.
 */
export function ChannelList({
  guild,
  dms,
  personas,
  goals,
  live,
  activeId,
  unread,
  onSelect,
}: {
  guild: Guild | undefined;
  dms: Channel[];
  personas: Persona[];
  goals: GoalSession[];
  live: Record<string, PersonaStatusEntry>;
  activeId: string;
  unread: Record<string, UnreadCount>;
  onSelect: (channel: Channel) => void;
}) {
  const rooms = (guild?.channels ?? []).filter((c) => c.kind !== "log");
  const logs = (guild?.channels ?? []).filter((c) => c.kind === "log");
  const byId = new Map(personas.map((p) => [p.id, p]));

  return (
    <aside className="flex w-[210px] shrink-0 flex-col overflow-y-auto border-r border-[var(--border)] bg-[var(--surface-2)]">
      <div className="border-b border-[var(--border)] px-3 py-2.5">
        <h2 className="truncate text-[12px] font-semibold text-[var(--text)]">
          {guild?.name || "Teams"}
        </h2>
      </div>

      <Section label="Channels">
        {rooms.map((c) => (
          <ChannelRow
            key={c.id}
            channel={c}
            active={activeId === c.id}
            unread={unread[c.channelId]}
            onSelect={() => onSelect(c)}
          />
        ))}
      </Section>

      {guild?.kind === "project" && (
        <Section label="Live logs">
          {logs.length === 0 ? (
            <Note>Nobody is assigned to this project yet.</Note>
          ) : (
            logs.map((c) => {
              const p = c.personaId ? byId.get(c.personaId) : undefined;
              const st = p ? memberLive(p, live, goals) : null;
              return (
                <ChannelRow
                  key={c.id}
                  channel={c}
                  active={activeId === c.id}
                  unread={unread[c.channelId]}
                  status={st?.phase}
                  enabled={p?.enabled !== false}
                  onSelect={() => onSelect(c)}
                />
              );
            })
          )}
        </Section>
      )}

      {guild?.kind === "company" && (
        <Section label="Direct messages">
          {dms.length === 0 ? (
            <Note>No agents yet. Hire them in Settings &gt; Agents.</Note>
          ) : (
            dms.map((c) => {
              const p = c.personaId ? byId.get(c.personaId) : undefined;
              const st = p ? memberLive(p, live, goals) : null;
              return (
                <ChannelRow
                  key={c.id}
                  channel={c}
                  active={activeId === c.id}
                  unread={unread[c.channelId]}
                  status={st?.phase}
                  enabled={p?.enabled !== false}
                  onSelect={() => onSelect(c)}
                />
              );
            })
          )}
        </Section>
      )}
    </aside>
  );
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <div className="px-3 pt-3 pb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
        {label}
      </div>
      <div className="px-1 pb-2">{children}</div>
    </div>
  );
}

function Note({ children }: { children: ReactNode }) {
  return <p className="px-3 py-3 text-[11px] leading-relaxed text-[var(--text-faint)]">{children}</p>;
}

export function ChannelRow({
  channel,
  active,
  unread,
  onSelect,
  status,
  enabled = true,
}: {
  channel: Channel;
  active: boolean;
  unread?: UnreadCount;
  onSelect: () => void;
  status?: string;
  enabled?: boolean;
}) {
  const unseen = (unread?.count ?? 0) > 0 && !active;
  const mentions = unread?.mentions ?? 0;
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`mb-0.5 flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-[12px] ${
        active
          ? "bg-[var(--accent-muted)] text-[var(--text)]"
          : unseen
            ? "font-medium text-[var(--text)] hover:bg-[var(--border)]/40"
            : "text-[var(--text-dim)] hover:bg-[var(--border)]/40 hover:text-[var(--text)]"
      } ${enabled ? "" : "opacity-50"}`}
    >
      {channel.kind === "dm" ? (
        <span className="relative flex h-4 w-4 shrink-0 items-center justify-center">
          <span
            className="flex h-4 w-4 items-center justify-center rounded-full text-[8px] font-semibold text-[var(--bg)]"
            style={{ background: avatarColor(channel.personaId || channel.title) }}
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
      ) : channel.kind === "log" ? (
        <span className="relative flex h-4 w-4 shrink-0 items-center justify-center">
          <TerminalIcon size={12} className="text-[var(--text-faint)]" />
          {status && status !== "idle" && (
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
      {mentions > 0 && (
        <span className="shrink-0 rounded-sm bg-red-500/90 px-1 text-[9px] font-semibold leading-[14px] text-white">
          {badge(mentions)}
        </span>
      )}
    </button>
  );
}
