import { UsersIcon } from "../../../src/components/icons";
import { avatarColor, initials, type Guild } from "./channels";
import { badge, type UnreadCount } from "./unread";

/**
 * The server rail: one tile per project, plus the company tile at the top.
 *
 * Squares rather than circles, and one accent, because the rail sits beside a terminal
 * all day. The tile carries only two signals -- something is new, and something names
 * you -- since a rail that shows a number per project is unreadable at a glance, which
 * is the only thing a rail is for.
 */
export function GuildRail({
  guilds,
  activeId,
  unread,
  onSelect,
}: {
  guilds: Guild[];
  activeId: string;
  unread: Record<string, UnreadCount>;
  onSelect: (id: string) => void;
}) {
  return (
    <nav
      aria-label="Projects"
      className="flex w-[56px] shrink-0 flex-col items-center gap-1 overflow-y-auto border-r border-[var(--border)] bg-[var(--surface-2)] py-2"
    >
      {guilds.map((g) => {
        const active = g.id === activeId;
        const count = unread[g.id] ?? { count: 0, mentions: 0 };
        return (
          <button
            key={g.id}
            type="button"
            onClick={() => onSelect(g.id)}
            title={g.name}
            aria-current={active ? "page" : undefined}
            className="relative flex h-10 w-10 shrink-0 items-center justify-center"
          >
            {/* The active marker is the rail's only strong accent. */}
            <span
              className={`absolute left-[-8px] w-[3px] rounded-r bg-[var(--accent)] transition-all ${
                active ? "h-6 opacity-100" : count.count > 0 ? "h-2 opacity-60" : "h-0 opacity-0"
              }`}
            />
            <span
              className={`flex h-9 w-9 items-center justify-center rounded-md border text-[11px] font-semibold ${
                active
                  ? "border-[var(--accent)] bg-[var(--accent-muted)] text-[var(--text)]"
                  : "border-[var(--border)] bg-[var(--bg)] text-[var(--text-dim)] hover:border-[var(--text-faint)] hover:text-[var(--text)]"
              }`}
              style={
                g.kind === "project" && !active
                  ? { color: avatarColor(g.id), borderColor: "var(--border)" }
                  : undefined
              }
            >
              {g.kind === "company" ? <UsersIcon size={16} /> : initials(g.name)}
            </span>
            {count.mentions > 0 ? (
              <span className="absolute -bottom-0.5 -right-0.5 min-w-[15px] rounded-sm bg-red-500/90 px-1 text-center text-[9px] font-semibold leading-[15px] text-white">
                {badge(count.mentions)}
              </span>
            ) : count.count > 0 ? (
              <span className="absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full border border-[var(--surface-2)] bg-[var(--text-dim)]" />
            ) : null}
          </button>
        );
      })}
    </nav>
  );
}
