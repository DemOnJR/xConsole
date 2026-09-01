import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  onAgentMessage,
  type AgentMessage,
  type GoalSession,
  type Persona,
  type Workspace,
} from "../../../src/lib/tauri";
import { CloseIcon, UsersIcon } from "../../../src/components/icons";
import { usePersonaStatusStore } from "../../../src/stores/personaStatusStore";
import { memberLive, phaseColor } from "./status";

const COMPANY = "__company__";

function nameOf(personas: Persona[], id?: string | null): string {
  if (!id) return "You";
  return personas.find((p) => p.id === id)?.name || "Unknown";
}

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
  const [teamId, setTeamId] = useState<string>(COMPANY);
  const [memberId, setMemberId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = usePersonaStatusStore((s) => s.byKey);
  const scroller = useRef<HTMLDivElement>(null);

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
    const ws = teamId === COMPANY ? null : teamId;
    let live = true;
    api
      .listAgentMessages(null, ws, 300)
      .then((m) => {
        if (!live) return;
        setMessages(ws ? m.filter((x) => x.workspace_id === ws) : m.filter((x) => !x.workspace_id));
      })
      .catch(() => live && setMessages([]));
    return () => {
      live = false;
    };
  }, [teamId]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onAgentMessage((msg) => {
      setMessages((prev) => {
        if (prev.some((m) => m.id === msg.id)) return prev;
        const ws = teamId === COMPANY ? null : teamId;
        if (ws && msg.workspace_id !== ws) return prev;
        if (!ws && msg.workspace_id) return prev;
        return [...prev, msg];
      });
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [teamId]);

  useEffect(() => {
    const el = scroller.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages.length, memberId]);

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

  const teams = useMemo(() => {
    const ids = new Set(personas.map((p) => p.workspace_id).filter(Boolean) as string[]);
    const named = workspaces.filter((w) => ids.has(w.id));
    return [{ id: COMPANY, name: "Company-wide" }, ...named.map((w) => ({ id: w.id, name: w.name }))];
  }, [personas, workspaces]);

  const members = useMemo(() => {
    if (teamId === COMPANY) return personas.filter((p) => !p.workspace_id);
    return personas.filter((p) => p.workspace_id === teamId);
  }, [personas, teamId]);

  const visibleMessages = useMemo(() => {
    if (!memberId) return messages;
    return messages.filter((m) => m.from_id === memberId || m.to_id === memberId);
  }, [messages, memberId]);

  const selected = members.find((p) => p.id === memberId) || null;

  const send = async () => {
    const body = draft.trim();
    if (!body || sending) return;
    setSending(true);
    setError(null);
    try {
      await api.postAgentMessage(
        body,
        memberId,
        teamId === COMPANY ? null : teamId,
        memberId ? "request" : "note",
      );
      setDraft("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-[var(--bg)] text-[var(--text)]">
      <header className="flex items-center gap-3 border-b border-[var(--border)] px-4 py-3">
        <UsersIcon size={18} className="text-[var(--text-faint)]" />
        <div className="min-w-0 flex-1">
          <h1 className="text-[13px] font-semibold tracking-wide">Teams</h1>
          <p className="truncate text-[11px] text-[var(--text-faint)]">
            What each agent is doing, and what they say to each other
          </p>
        </div>
        {onClose && (
          <button
            type="button"
            className="xc-icon-btn"
            data-tooltip="Close"
            onClick={onClose}
          >
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
        <aside className="flex w-[200px] shrink-0 flex-col border-r border-[var(--border)]">
          <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
            Teams
          </div>
          <div className="flex-1 overflow-y-auto px-1 pb-2">
            {teams.map((t) => {
              const count =
                t.id === COMPANY
                  ? personas.filter((p) => !p.workspace_id).length
                  : personas.filter((p) => p.workspace_id === t.id).length;
              const active = teamId === t.id;
              return (
                <button
                  key={t.id}
                  type="button"
                  onClick={() => {
                    setTeamId(t.id);
                    setMemberId(null);
                  }}
                  className={`mb-0.5 flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-[12px] ${
                    active
                      ? "bg-[var(--accent-muted)] text-[var(--text)]"
                      : "text-[var(--text-faint)] hover:bg-[var(--border)]/40 hover:text-[var(--text)]"
                  }`}
                >
                  <span className="truncate">{t.name}</span>
                  <span className="font-mono text-[10px] text-[var(--text-faint)]">{count}</span>
                </button>
              );
            })}
          </div>
        </aside>

        <section className="flex w-[260px] shrink-0 flex-col border-r border-[var(--border)]">
          <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
            People
          </div>
          <div className="flex-1 overflow-y-auto px-1 pb-2">
            {members.length === 0 ? (
              <p className="px-2 py-6 text-center text-[11px] text-[var(--text-faint)]">
                No agents on this team. Hire them in Settings → Agents.
              </p>
            ) : (
              members.map((p) => {
                const st = memberLive(p, live, goals);
                const active = memberId === p.id;
                return (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setMemberId((cur) => (cur === p.id ? null : p.id))}
                    className={`mb-0.5 flex w-full flex-col gap-0.5 rounded px-2 py-1.5 text-left ${
                      active
                        ? "bg-[var(--accent-muted)]"
                        : "hover:bg-[var(--border)]/40"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className="h-1.5 w-1.5 shrink-0 rounded-full"
                        style={{ background: phaseColor(st.phase) }}
                      />
                      <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--text)]">
                        {p.name}
                      </span>
                      {!p.enabled && (
                        <span className="text-[10px] text-[var(--text-faint)]">off</span>
                      )}
                    </div>
                    <div className="truncate pl-3.5 font-mono text-[10px] text-[var(--text-faint)]">
                      {st.label}
                    </div>
                    {st.task && (
                      <div className="truncate pl-3.5 text-[10px] text-[var(--text-faint)]">
                        {st.task}
                      </div>
                    )}
                  </button>
                );
              })
            )}
          </div>
        </section>

        <section className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-baseline gap-2 border-b border-[var(--border)] px-4 py-2">
            <h2 className="text-[12px] font-semibold">
              {selected ? `${selected.name}` : "Team chat"}
            </h2>
            <span className="truncate text-[11px] text-[var(--text-faint)]">
              {selected
                ? selected.role || "Direct messages with this agent"
                : "Everyone on this team"}
            </span>
          </div>

          {selected && (
            <MemberBrief persona={selected} goals={goals} live={live} />
          )}

          <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
            {visibleMessages.length === 0 ? (
              <p className="py-10 text-center text-[11px] text-[var(--text-faint)]">
                Nothing said yet on this thread.
              </p>
            ) : (
              visibleMessages.map((m) => (
                <div key={m.id} className="mb-3">
                  <div className="mb-0.5 flex items-baseline gap-2 text-[10px] text-[var(--text-faint)]">
                    <span className="font-semibold text-[var(--text)]">
                      {nameOf(personas, m.from_id)}
                    </span>
                    <span>
                      {m.kind}
                      {m.to_id ? ` → ${nameOf(personas, m.to_id)}` : ""}
                    </span>
                    <span className="ml-auto font-mono">{clock(m.created_at)}</span>
                  </div>
                  <p className="whitespace-pre-wrap text-[12px] leading-relaxed text-[var(--text)]">
                    {m.body}
                  </p>
                </div>
              ))
            )}
          </div>

          <form
            className="flex gap-2 border-t border-[var(--border)] px-3 py-2"
            onSubmit={(e) => {
              e.preventDefault();
              void send();
            }}
          >
            <input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={
                selected
                  ? `Message ${selected.name}…`
                  : "Message the team…"
              }
              className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)]"
            />
            <button
              type="submit"
              disabled={sending || !draft.trim()}
              className="rounded border border-[var(--border)] px-3 py-1.5 text-[11px] text-[var(--text)] disabled:opacity-40 hover:border-[var(--border-strong)]"
            >
              Send
            </button>
          </form>
        </section>
      </div>
    </div>
  );
}

function MemberBrief({
  persona,
  goals,
  live,
}: {
  persona: Persona;
  goals: GoalSession[];
  live: ReturnType<typeof usePersonaStatusStore.getState>["byKey"];
}) {
  const st = memberLive(persona, live, goals);
  const mine = goals
    .filter((g) => g.persona_id === persona.id)
    .sort((a, b) => (b.updated_at || "").localeCompare(a.updated_at || ""))
    .slice(0, 4);
  return (
    <div className="border-b border-[var(--border)] px-4 py-2">
      <div className="flex items-center gap-2 text-[11px]">
        <span
          className="h-1.5 w-1.5 rounded-full"
          style={{ background: phaseColor(st.phase) }}
        />
        <span className="font-mono text-[var(--text)]">{st.label}</span>
        {persona.role && (
          <span className="text-[var(--text-faint)]">· {persona.role}</span>
        )}
      </div>
      {mine.length > 0 && (
        <ul className="mt-1.5 space-y-0.5">
          {mine.map((g) => (
            <li key={g.id} className="flex gap-2 font-mono text-[10px] text-[var(--text-faint)]">
              <span className="w-16 shrink-0 uppercase">{g.status}</span>
              <span className="truncate text-[var(--text)]">{g.title}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
