import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  onAgentMessage,
  type AgentMessage,
  type Persona,
  type PersonaInput,
  type ProjectHistory,
} from "../../../lib/tauri";
import { useVpsStore } from "../../../stores/vpsStore";
import { useWorkspaceStore } from "../../../stores/workspaceStore";
import { useSettingsStore } from "../../../stores/settingsStore";
import { Button, Field, SectionHeader, Select, TextArea, TextInput, Toggle } from "../ui";
import { BotIcon, CloseIcon, PlusIcon, TrashIcon } from "../../icons";

/** A persona that has not been saved yet. */
function blankPersona(): PersonaInput {
  return {
    name: "",
    role: "",
    instructions: "",
    targets: [],
    safety_mode: null,
    provider_id: null,
    model: null,
    enabled: true,
    reports_to: null,
  };
}

function toInput(p: Persona): PersonaInput {
  return {
    id: p.id,
    name: p.name,
    role: p.role,
    instructions: p.instructions,
    targets: p.targets ?? [],
    safety_mode: p.safety_mode ?? null,
    provider_id: p.provider_id ?? null,
    model: p.model ?? null,
    enabled: p.enabled,
    reports_to: p.reports_to ?? null,
  };
}

/**
 * Build the reporting tree for display.
 *
 * Anyone whose manager is missing (deleted, or disabled) is appended at the root
 * instead of being dropped — an agent you cannot see is an agent you cannot fix.
 */
function orgRows(personas: Persona[]): { persona: Persona; depth: number }[] {
  const rows: { persona: Persona; depth: number }[] = [];
  const seen = new Set<string>();
  const childrenOf = (id: string | null) =>
    personas.filter((p) => (p.reports_to || null) === id);

  const walk = (p: Persona, depth: number) => {
    if (seen.has(p.id) || depth > 12) return;
    seen.add(p.id);
    rows.push({ persona: p, depth });
    for (const child of childrenOf(p.id)) walk(child, depth + 1);
  };

  for (const top of childrenOf(null)) walk(top, 0);
  for (const orphan of personas) {
    if (!seen.has(orphan.id)) rows.push({ persona: orphan, depth: 0 });
  }
  return rows;
}

export function AgentsSection() {
  const [personas, setPersonas] = useState<Persona[]>([]);
  const [draft, setDraft] = useState<PersonaInput | null>(null);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const vps = useVpsStore((s) => s.vpsList);
  const providers = useSettingsStore((s) => s.providers);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const activeWorkspace = useWorkspaceStore((s) => s.activeId);
  // Defaults to the project that is open, because that is almost always what the
  // question is about. "" means all of them, which is only useful for a survey.
  const [project, setProject] = useState<string>(activeWorkspace ?? "");
  const [history, setHistory] = useState<ProjectHistory | null>(null);

  const load = useCallback(async () => {
    const [list, msgs] = await Promise.all([
      api.listPersonas().catch(() => [] as Persona[]),
      api
        .listAgentMessages(null, project || null, 200)
        .catch(() => [] as AgentMessage[]),
    ]);
    setPersonas(list);
    setMessages(msgs);
    setHistory(
      project ? await api.projectHistory(project, 200).catch(() => null) : null,
    );
  }, [project]);

  useEffect(() => {
    void load();
  }, [load]);

  // Watching agents talk is the point, so the feed is live rather than a snapshot
  // the user has to remember to refresh.
  useEffect(() => {
    const un = onAgentMessage((msg) => {
      // A live message from another project must not appear in a filtered view — that
      // is the mixing this filter exists to stop.
      if (project && msg.workspace_id !== project) return;
      setMessages((prev) => [...prev, msg]);
    });
    return () => {
      void un.then((f) => f());
    };
  }, [project]);

  const nameOf = useMemo(() => {
    const map = new Map(personas.map((p) => [p.id, p.name]));
    return (id?: string | null) => (id ? map.get(id) ?? "(deleted)" : "You");
  }, [personas]);

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setError(null);
    try {
      await api.savePersona(draft);
      setDraft(null);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (p: Persona) => {
    await api.deletePersona(p.id).catch((e) => setError(String(e)));
    if (draft?.id === p.id) setDraft(null);
    await load();
  };

  const rows = orgRows(personas);

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Agents"
        description="Named agents that take work in the background and report back. Give one a manager and it escalates to them instead of interrupting you — only an agent that reports to you can reach you directly."
        action={
          <Button variant="primary" onClick={() => setDraft(blankPersona())}>
            <PlusIcon size={13} />
            New agent
          </Button>
        }
      />

      {error && (
        <div className="border border-red-500/40 bg-red-500/10 px-3 py-2 text-[11px] text-red-300">
          {error}
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <div className="space-y-2">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">
            Org chart
          </h3>
          {rows.length === 0 ? (
            <p className="border border-dashed border-[var(--border)] px-3 py-6 text-center text-[11px] text-gray-500">
              No agents yet. Create one and the main agent can hand work to it.
            </p>
          ) : (
            <div className="divide-y divide-[var(--border)] border border-[var(--border)]">
              {rows.map(({ persona, depth }) => (
                <div
                  key={persona.id}
                  className="group flex items-center gap-2 bg-[var(--surface)] px-3 py-2 hover:bg-[var(--surface-hover)]"
                  style={{ paddingLeft: 12 + depth * 16 }}
                >
                  <BotIcon
                    size={13}
                    className={persona.enabled ? "text-[var(--text-dim)]" : "text-[var(--text-faint)]"}
                  />
                  <button
                    type="button"
                    onClick={() => setDraft(toInput(persona))}
                    className="min-w-0 flex-1 text-left"
                  >
                    <span
                      className={`text-xs font-medium ${
                        persona.enabled ? "text-gray-200" : "text-gray-500 line-through"
                      }`}
                    >
                      {persona.name}
                    </span>
                    {persona.role && (
                      <span className="ml-2 truncate text-[11px] text-gray-500">
                        {persona.role}
                      </span>
                    )}
                  </button>
                  {!persona.reports_to && (
                    <span
                      className="shrink-0 border border-[var(--border-strong)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-gray-400"
                      title="Reports to you — this agent can message you directly"
                    >
                      reports to you
                    </span>
                  )}
                  <button
                    type="button"
                    onClick={() => void remove(persona)}
                    title={`Delete ${persona.name}`}
                    className="shrink-0 p-1 text-[var(--text-faint)] opacity-0 transition group-hover:opacity-100 hover:text-red-400"
                  >
                    <TrashIcon size={13} />
                  </button>
                </div>
              ))}
            </div>
          )}

          <div className="flex items-center gap-2 pt-4">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">
              What they said to each other
            </h3>
            <div className="ml-auto w-44">
              <Select value={project} onChange={(e) => setProject(e.target.value)}>
                <option value="">All projects</option>
                {workspaces.map((w) => (
                  <option key={w.id} value={w.id}>
                    {w.name}
                  </option>
                ))}
              </Select>
            </div>
          </div>
          {!project && (
            <p className="text-[11px] text-gray-500">
              Showing every project at once. Pick one to read its thread on its own,
              alongside what was delegated, changed and committed.
            </p>
          )}
          <ConversationFeed messages={messages} nameOf={nameOf} />
          {history && <ProjectRecord history={history} nameOf={nameOf} />}
        </div>

        <div>
          {draft ? (
            <PersonaEditor
              draft={draft}
              personas={personas}
              vps={vps}
              providers={providers}
              saving={saving}
              onChange={setDraft}
              onSave={save}
              onCancel={() => {
                setDraft(null);
                setError(null);
              }}
            />
          ) : (
            <p className="border border-dashed border-[var(--border)] px-3 py-6 text-center text-[11px] text-gray-500">
              Select an agent to edit it, or create one.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * One project's record beside its conversation.
 *
 * The conversation alone answers "what did they say"; this answers "and what came of
 * it". Keeping the four together is the point — reading them as separate screens means
 * correlating tasks, edits and commits by timestamp, which is the work the user was
 * doing by hand before there was a project to file them under.
 */
function ProjectRecord({
  history,
  nameOf,
}: {
  history: ProjectHistory;
  nameOf: (id?: string | null) => string;
}) {
  return (
    <div className="mt-4 space-y-4">
      <div className="flex items-baseline gap-2">
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">
          {history.name}
        </h3>
        {history.branch && (
          <span className="font-mono text-[10px] text-gray-500">{history.branch}</span>
        )}
        {history.location && (
          <span className="truncate font-mono text-[10px] text-gray-600">
            {history.location}
          </span>
        )}
      </div>

      <RecordList
        title="Delegated"
        empty="Nothing has been delegated on this project."
        rows={history.tasks.slice(0, 12).map((t) => ({
          key: t.id,
          // Status is the one thing here worth scanning for, so it leads.
          lead: t.status,
          text: t.title,
          tail: `${t.cycles} cycles${t.persona_id ? ` · ${nameOf(t.persona_id)}` : ""}`,
        }))}
      />

      <RecordList
        title="Files changed"
        empty="No files changed on this project yet."
        rows={history.changes.slice(0, 12).map((c) => ({
          key: c.id,
          lead: c.is_new ? "new" : "edit",
          text: c.path,
          tail: c.label,
        }))}
      />

      <RecordList
        title="Commits"
        empty={history.git_note ?? "No commits yet."}
        rows={history.commits.slice(0, 12).map((c) => ({
          key: c.sha,
          lead: c.sha,
          text: c.subject,
          tail: c.author,
        }))}
      />
    </div>
  );
}

function RecordList({
  title,
  empty,
  rows,
}: {
  title: string;
  empty: string;
  rows: { key: string; lead: string; text: string; tail: string }[];
}) {
  return (
    <div className="space-y-1.5">
      <h4 className="text-[10px] font-semibold uppercase tracking-wider text-gray-500">
        {title}
      </h4>
      {rows.length === 0 ? (
        <p className="border border-dashed border-[var(--border)] px-3 py-3 text-center text-[11px] text-gray-500">
          {empty}
        </p>
      ) : (
        <div className="divide-y divide-[var(--border)] border border-[var(--border)] bg-[var(--surface-2)]">
          {rows.map((r) => (
            <div key={r.key} className="flex items-baseline gap-2 px-3 py-1.5">
              <span className="shrink-0 font-mono text-[10px] text-gray-500">{r.lead}</span>
              <span className="min-w-0 flex-1 truncate text-[11px] text-gray-300">{r.text}</span>
              <span className="shrink-0 truncate font-mono text-[10px] text-gray-600">
                {r.tail}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ConversationFeed({
  messages,
  nameOf,
}: {
  messages: AgentMessage[];
  nameOf: (id?: string | null) => string;
}) {
  const endRef = useRef<HTMLDivElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);

  // Follow the tail only while the user is already at the bottom, so a new message
  // does not yank them away from something they scrolled up to read.
  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (atBottom) endRef.current?.scrollIntoView({ block: "end" });
  }, [messages]);

  if (messages.length === 0) {
    return (
      <p className="border border-dashed border-[var(--border)] px-3 py-6 text-center text-[11px] text-gray-500">
        Nothing yet. Messages appear here as agents ask each other things and report
        back.
      </p>
    );
  }

  return (
    <div
      ref={scrollerRef}
      className="max-h-72 overflow-y-auto border border-[var(--border)] bg-[var(--surface-2)]"
    >
      {messages.map((m) => (
        <div key={m.id} className="border-b border-[var(--border)] px-3 py-2 last:border-b-0">
          <div className="flex items-baseline gap-1.5 font-mono text-[10px] text-gray-500">
            <span className="text-gray-300">{nameOf(m.from_id)}</span>
            <span>-&gt;</span>
            <span className="text-gray-300">{nameOf(m.to_id)}</span>
            {/* The one place colour earns its keep here: an escalation that reached
                the user is the message that actually wants attention. */}
            <span
              className={
                m.kind === "report" && !m.to_id
                  ? "text-[var(--warning)]"
                  : "text-gray-600"
              }
            >
              [{m.kind}]
            </span>
          </div>
          <div className="mt-0.5 whitespace-pre-wrap text-[11px] leading-relaxed text-gray-300">
            {m.body}
          </div>
        </div>
      ))}
      <div ref={endRef} />
    </div>
  );
}

function PersonaEditor({
  draft,
  personas,
  vps,
  providers,
  saving,
  onChange,
  onSave,
  onCancel,
}: {
  draft: PersonaInput;
  personas: Persona[];
  vps: { id: string; name: string }[];
  providers: { id: string; name: string }[];
  saving: boolean;
  onChange: (next: PersonaInput) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const set = <K extends keyof PersonaInput>(key: K, value: PersonaInput[K]) =>
    onChange({ ...draft, [key]: value });

  const toggleTarget = (id: string) =>
    set(
      "targets",
      draft.targets.includes(id)
        ? draft.targets.filter((t) => t !== id)
        : [...draft.targets, id],
    );

  // An agent cannot manage itself; deeper loops are refused by the backend, which
  // is the only place that can see the whole chart.
  const managerOptions = personas.filter((p) => p.id !== draft.id);

  return (
    <div className="space-y-4 border border-[var(--border)] bg-[var(--surface)] p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-300">
          {draft.id ? `Edit ${draft.name || "agent"}` : "New agent"}
        </h3>
        <button
          type="button"
          onClick={onCancel}
          className="p-1 text-[var(--text-faint)] hover:text-gray-200"
        >
          <CloseIcon size={13} />
        </button>
      </div>

      <Field label="Name" hint="What you and the other agents call it — Ada, CEO, night-shift.">
        <TextInput
          value={draft.name}
          autoFocus
          onChange={(e) => set("name", e.target.value)}
          placeholder="Ada"
        />
      </Field>

      <Field
        label="Role"
        hint="One line on what it is for. Work is routed by this, so be specific: 'nginx, TLS and firewalls' beats 'helps out'."
      >
        <TextInput
          value={draft.role}
          onChange={(e) => set("role", e.target.value)}
          placeholder="infrastructure — nginx, TLS, firewall, disk"
        />
      </Field>

      <Field
        label="Reports to"
        hint="Escalations go here instead of to you. Leave as You and this agent can message you directly."
      >
        <Select
          value={draft.reports_to ?? ""}
          onChange={(e) => set("reports_to", e.target.value || null)}
        >
          <option value="">You</option>
          {managerOptions.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>

      <Field label="Standing instructions" hint="Always in this agent's prompt. How it should work, what it must never do.">
        <TextArea
          value={draft.instructions}
          rows={5}
          onChange={(e) => set("instructions", e.target.value)}
          placeholder={"Check systemctl status before restarting anything.\nNever touch the database host."}
        />
      </Field>

      <Field label="Servers" hint="Where it works unless a task says otherwise.">
        {vps.length === 0 ? (
          <p className="text-[11px] text-gray-500">No servers configured yet.</p>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {vps.map((v) => {
              const on = draft.targets.includes(v.id);
              return (
                <button
                  key={v.id}
                  type="button"
                  onClick={() => toggleTarget(v.id)}
                  className={`border px-2 py-1 text-[11px] transition ${
                    on
                      ? "border-[var(--accent)] bg-[var(--accent-muted)] text-gray-100"
                      : "border-[var(--border)] text-gray-400 hover:border-[var(--border-strong)]"
                  }`}
                >
                  {v.name}
                </button>
              );
            })}
          </div>
        )}
      </Field>

      <div className="grid grid-cols-2 gap-3">
        <Field label="Trust" hint="Overrides the global safety mode.">
          <Select
            value={draft.safety_mode ?? ""}
            onChange={(e) => set("safety_mode", e.target.value || null)}
          >
            <option value="">Use global</option>
            <option value="approve">Ask every command</option>
            <option value="allowlist">Auto-run read-only</option>
            <option value="full">Run anything</option>
          </Select>
        </Field>
        <Field label="Model" hint="Routine work need not use your best model.">
          <Select
            value={draft.provider_id ?? ""}
            onChange={(e) => set("provider_id", e.target.value || null)}
          >
            <option value="">Use active provider</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
        </Field>
      </div>

      <div className="flex items-center gap-3 border-t border-[var(--border)] pt-3">
        <Button variant="primary" onClick={onSave} disabled={saving || !draft.name.trim()}>
          {saving ? "Saving…" : "Save"}
        </Button>
        <Button onClick={onCancel}>Cancel</Button>
        <div className="ml-auto">
          <Toggle
            checked={draft.enabled}
            onChange={(v) => set("enabled", v)}
            label={draft.enabled ? "Active" : "Disabled"}
          />
        </div>
      </div>
    </div>
  );
}
