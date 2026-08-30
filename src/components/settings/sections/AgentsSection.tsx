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
import { BotIcon, CloseIcon, ICON, PlusIcon, TrashIcon } from "../../icons";

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
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const saveTimer = useRef<number | null>(null);
  /** Which row the editor is on. A new agent gets a temporary key until it has an id. */
  const editingKey = useRef<string>("");
  /**
   * A create is in flight.
   *
   * Until it returns there is no id, so a second autosave firing in the meantime would
   * try to create the same agent again — and the backend, quite rightly, refuses a
   * duplicate name. Nothing is lost, but the user gets an error for typing quickly.
   */
  const creating = useRef(false);
  /** The draft as last typed, so a save deferred by a create can pick it back up. */
  const latest = useRef<PersonaInput | null>(null);
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

  /**
   * Edits save themselves.
   *
   * The Save button closed the editor and reloaded the whole list, so changing two
   * settings on one agent meant: save, watch it collapse, find the agent again, scroll
   * back down, change the second thing, save, collapse. The list is not reloaded either
   * — the saved row is swapped in place, which is what keeps the scroll position and
   * stops the panel jumping under the cursor.
   */
  const commit = useCallback(async (input: PersonaInput, key: string) => {
    // Dropping the edit would be worse than the duplicate: the user stops typing and
    // their last keystrokes are simply gone. Defer instead — the create is about to
    // return an id, and `finally` picks the latest draft back up.
    if (!input.id) {
      if (creating.current) return;
      creating.current = true;
    }
    setSaving(true);
    setError(null);
    try {
      const saved = await api.savePersona(input);
      // Adopt the new id, or the next keystroke creates a second agent instead of
      // updating this one. Guarded by the editing key: if the user moved to a different
      // agent while this was in flight, the id belongs to a row they are no longer on.
      if (editingKey.current === key) {
        setDraft((d) => (d ? { ...d, id: saved.id } : d));
        if (latest.current) latest.current = { ...latest.current, id: saved.id };
      }
      setPersonas((list) =>
        list.some((p) => p.id === saved.id)
          ? list.map((p) => (p.id === saved.id ? saved : p))
          : [...list, saved],
      );
      setSavedAt(Date.now());
    } catch (e) {
      setError(String(e));
    } finally {
      const wasCreate = !input.id;
      creating.current = false;
      setSaving(false);
      // Anything typed while the create was in flight now has an id to save against.
      if (wasCreate && editingKey.current === key) {
        const now = latest.current;
        if (now && now.id && JSON.stringify({ ...now, id: null }) !== JSON.stringify({ ...input, id: null })) {
          void commitRef.current?.(now, key);
        }
      }
    }
  }, []);
  // `commit` refers to itself for the deferred retry above; a ref breaks the cycle
  // without making the callback depend on its own identity.
  const commitRef = useRef<typeof commit | null>(null);
  commitRef.current = commit;

  const edit = (next: PersonaInput) => {
    setDraft(next);
    latest.current = next;
    setError(null);
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    // A nameless agent is refused by the backend, so there is nothing to save yet —
    // and flashing that refusal while somebody types the first letter would be noise.
    if (!next.name.trim()) return;
    const key = editingKey.current;
    saveTimer.current = window.setTimeout(() => void commit(next, key), 600);
  };

  /** Open an agent for editing, flushing anything still pending on the previous one. */
  const open = (input: PersonaInput | null, key: string) => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
      // Switching away must not drop the last keystroke.
      if (draft?.name.trim()) void commit(draft, editingKey.current);
    }
    editingKey.current = key;
    setDraft(input);
    setError(null);
  };

  const remove = async (p: Persona) => {
    await api.deletePersona(p.id).catch((e) => setError(String(e)));
    if (draft?.id === p.id) open(null, "");
    setPersonas((list) => list.filter((x) => x.id !== p.id));
  };

  const rows = orgRows(personas);

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Agents"
        description="Named agents that take work in the background and report back. Give one a manager and it escalates to them instead of interrupting you — only an agent that reports to you can reach you directly."
        action={
          <Button variant="primary" onClick={() => open(blankPersona(), `new-${Date.now()}`)}>
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

      {/*
        One layout, always. The panel used to swap between a full-width list and a
        narrow list + editor, so opening an agent moved every row out from under the
        cursor. The list is sticky, so the team stays reachable while the editor scrolls.
      */}
      <div className="grid gap-5 lg:grid-cols-[260px_minmax(0,1fr)] lg:items-start">
        <div className="lg:sticky lg:top-4">
          <div className="mb-1.5 flex items-baseline justify-between">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">
              Team
            </h3>
            <span className="font-mono text-[10px] text-gray-600">{rows.length}</span>
          </div>
          {rows.length === 0 ? (
            <p className="border border-dashed border-[var(--border)] px-3 py-6 text-center text-[11px] text-gray-500">
              No agents yet.
            </p>
          ) : (
            <div className="max-h-[calc(100vh-15rem)] divide-y divide-[var(--border)] overflow-y-auto border border-[var(--border)]">
              {rows.map(({ persona, depth }) => {
                const selected = draft?.id === persona.id;
                return (
                  <div
                    key={persona.id}
                    className={`group flex items-center gap-2 py-2 pr-2.5 transition ${
                      selected
                        ? "bg-[var(--accent-muted)]"
                        : "bg-[var(--surface)] hover:bg-[var(--surface-hover)]"
                    }`}
                    style={{ paddingLeft: 10 + depth * 12 }}
                  >
                    <BotIcon
                      size={ICON.small}
                      className={
                        persona.enabled
                          ? "shrink-0 text-[var(--text-dim)]"
                          : "shrink-0 text-[var(--text-faint)]"
                      }
                    />
                    <button
                      type="button"
                      onClick={() => open(toInput(persona), persona.id)}
                      className="min-w-0 flex-1 text-left"
                    >
                      <span
                        className={`block truncate text-xs font-medium ${
                          persona.enabled ? "text-gray-200" : "text-gray-500 line-through"
                        }`}
                      >
                        {persona.name}
                      </span>
                      {persona.role && (
                        <span className="block truncate text-[10px] text-gray-500">
                          {persona.role}
                        </span>
                      )}
                    </button>
                    {!persona.reports_to && (
                      <span
                        className="shrink-0 border border-[var(--border-strong)] px-1 py-0.5 font-mono text-[9px] uppercase text-gray-400"
                        title="Reports to you — this agent can message you directly"
                      >
                        you
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={() => void remove(persona)}
                      title={`Delete ${persona.name}`}
                      className="shrink-0 p-1 text-[var(--text-faint)] opacity-0 transition hover:text-red-400 group-hover:opacity-100"
                    >
                      <TrashIcon size={ICON.small} />
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        <div className="min-w-0">
          {draft ? (
            <>
              <div className="mb-1.5 flex items-baseline gap-2">
                <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">
                  {draft.id ? draft.name || "Agent" : "New agent"}
                </h3>
                {/* Where the Save button was. The change is already stored; this only
                    reports it, so nothing here is a thing left to do. */}
                <span className="font-mono text-[10px] text-gray-500">
                  {saving ? "Saving…" : savedAt ? "Saved" : "Changes save as you type"}
                </span>
                <button
                  type="button"
                  onClick={() => open(null, "")}
                  className="ml-auto p-1 text-[var(--text-faint)] transition hover:text-gray-200"
                  title="Close editor"
                >
                  <CloseIcon size={ICON.small} />
                </button>
              </div>
              <PersonaEditor
                draft={draft}
                personas={personas}
                vps={vps}
                providers={providers}
                onChange={edit}
              />
            </>
          ) : (
            <p className="border border-dashed border-[var(--border)] px-3 py-10 text-center text-[11px] text-gray-500">
              Pick an agent to edit it, or create one. Changes save as you type.
            </p>
          )}
        </div>
      </div>

      {/* Activity & Conversation Section */}
      <div className="space-y-3 border-t border-[var(--border)] pt-5">
        <div className="flex items-center justify-between">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-gray-400">
            What they said to each other
          </h3>
          <div className="w-52">
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

/**
 * A headed block of related fields.
 *
 * The editor was one column of eight unrelated controls, so finding "Trust" meant
 * reading all of them. Three groups — who it is, how it works, what it may reach —
 * make it scannable, and the last is the one worth pausing over.
 */
function EditorGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="space-y-3">
      <h4 className="border-b border-[var(--border)] pb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-500">
        {title}
      </h4>
      {children}
    </div>
  );
}

function PersonaEditor({
  draft,
  personas,
  vps,
  providers,
  onChange,
}: {
  draft: PersonaInput;
  personas: Persona[];
  vps: { id: string; name: string }[];
  providers: { id: string; name: string; model?: string | null }[];
  onChange: (next: PersonaInput) => void;
}) {
  const set = <K extends keyof PersonaInput>(key: K, value: PersonaInput[K]) =>
    onChange({ ...draft, [key]: value });

  // What an empty Model box actually means, named rather than left to be guessed.
  const providerModel =
    providers.find((p) => p.id === draft.provider_id)?.model?.trim() || "";

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
    // Title, save state and close live in the header above this, so the editor is only
    // the fields — one column of them, in the order somebody sets an agent up.
    <div className="space-y-5 border border-[var(--border)] bg-[var(--surface)] p-4">
      <EditorGroup title="Identity">
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

      </EditorGroup>

      <EditorGroup title="How it works">
      <Field label="Standing instructions" hint="Always in this agent's prompt. How it should work, what it must never do.">
        <TextArea
          value={draft.instructions}
          rows={5}
          onChange={(e) => set("instructions", e.target.value)}
          placeholder={"Check systemctl status before restarting anything.\nNever touch the database host."}
        />
      </Field>

      </EditorGroup>

      <EditorGroup title="Reach and trust">
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
        <Field label="Provider" hint="Which account or CLI this one runs through.">
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
        {/* Separate from the provider on purpose: two agents can share one account and
            still run on different models — the big one for whoever plans, a cheap one
            for whoever answers routine questions. */}
        <Field
          label="Model"
          hint={`Blank uses whatever the provider is set to${
            providerModel ? ` (${providerModel})` : ""
          }. Routine work need not use your best model.`}
        >
          <TextInput
            value={draft.model ?? ""}
            onChange={(e) => set("model", e.target.value || null)}
            placeholder={providerModel || "provider default"}
          />
        </Field>
      </div>

      </EditorGroup>

      <div className="flex items-center gap-3 border-t border-[var(--border)] pt-3">
        <Toggle
          checked={draft.enabled}
          onChange={(v) => set("enabled", v)}
          label={draft.enabled ? "Active — may be given work" : "Disabled"}
        />
      </div>
    </div>
  );
}
