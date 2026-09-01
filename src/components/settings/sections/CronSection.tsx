import { useEffect, useState } from "react";
import { useCronStore } from "../../../stores/cronStore";
import { useVpsStore } from "../../../stores/vpsStore";
import { useWorkspaceStore } from "../../../stores/workspaceStore";
import { dialog } from "../../../stores/dialogStore";
import { api, type CronJob, type CronJobInput, type Persona } from "../../../lib/tauri";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, Select, TextArea, TextInput, Toggle } from "../ui";

function emptyJob(): CronJobInput {
  return {
    name: "",
    schedule: "@every 1h",
    kind: "command",
    payload: "",
    targets_json: "[]",
    enabled: true,
    workspace_id: null,
    persona_id: null,
  };
}

function CronForm({
  initial,
  onClose,
}: {
  initial: CronJob | null;
  onClose: () => void;
}) {
  const save = useCronStore((s) => s.save);
  const vpsList = useVpsStore((s) => s.vpsList);
  const loadVps = useVpsStore((s) => s.load);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const [personas, setPersonas] = useState<Persona[]>([]);

  const [form, setForm] = useState<CronJobInput>(
    initial
      ? {
          id: initial.id,
          name: initial.name,
          schedule: initial.schedule,
          kind: initial.kind,
          payload: initial.payload,
          targets_json: initial.targets_json ?? "[]",
          enabled: initial.enabled,
          workspace_id: initial.workspace_id ?? null,
          persona_id: initial.persona_id ?? null,
        }
      : emptyJob(),
  );
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    loadVps();
    void api.listPersonas().then(setPersonas).catch(() => setPersonas([]));
  }, [loadVps]);

  const targets: string[] = (() => {
    try {
      return JSON.parse(form.targets_json || "[]");
    } catch {
      return [];
    }
  })();

  const patch = (p: Partial<CronJobInput>) => setForm((f) => ({ ...f, ...p }));
  const setTargets = (ids: string[]) => patch({ targets_json: JSON.stringify(ids) });
  const toggleTarget = (id: string) =>
    setTargets(targets.includes(id) ? targets.filter((t) => t !== id) : [...targets, id]);

  const submit = async () => {
    if (!form.name.trim() || !form.payload.trim()) return;
    setSaving(true);
    try {
      await save(form);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="w-[min(560px,92vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
        <h3 className="mb-4 text-sm font-semibold text-gray-100">
          {initial ? "Edit job" : "New cron job"}
        </h3>

        <Field label="Name">
          <TextInput
            value={form.name}
            onChange={(e) => patch({ name: e.target.value })}
            placeholder="Nightly disk check"
            autoFocus
          />
        </Field>

        <div className="flex gap-3">
          <div className="flex-1">
            <Field
              label="Schedule"
              hint="@every 5m · @hourly · @daily 03:00 · @weekly mon 09:00"
            >
              <TextInput
                value={form.schedule}
                onChange={(e) => patch({ schedule: e.target.value })}
                placeholder="@every 1h"
              />
            </Field>
          </div>
          <div className="w-40">
            <Field label="Type">
              <Select value={form.kind} onChange={(e) => patch({ kind: e.target.value })}>
                <option value="command">Command</option>
                <option value="prompt">Agent prompt</option>
              </Select>
            </Field>
          </div>
        </div>

        {form.kind === "prompt" && (
          <div className="flex gap-3">
            <div className="flex-1">
              <Field
                label="Project"
                hint="The run gets this project's brief, and files its work there."
              >
                <Select
                  value={form.workspace_id ?? ""}
                  onChange={(e) => patch({ workspace_id: e.target.value || null })}
                >
                  <option value="">No project</option>
                  {workspaces.map((w) => (
                    <option key={w.id} value={w.id}>
                      {w.name}
                    </option>
                  ))}
                </Select>
              </Field>
            </div>
            <div className="flex-1">
              <Field
                label="Runs as"
                hint="Under that agent's instructions and trust level. This is what makes a schedule a member of staff rather than a script."
              >
                <Select
                  value={form.persona_id ?? ""}
                  onChange={(e) => patch({ persona_id: e.target.value || null })}
                >
                  <option value="">The main agent</option>
                  {personas
                    .filter((p) => p.enabled)
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                        {p.role ? ` — ${p.role}` : ""}
                      </option>
                    ))}
                </Select>
              </Field>
            </div>
          </div>
        )}

        <Field
          label={form.kind === "command" ? "Command" : "Prompt"}
          hint={
            form.kind === "command"
              ? "Runs on each target, honoring its safety mode."
              : "Runs the full agent with these targets available."
          }
        >
          <TextArea
            value={form.payload}
            onChange={(e) => patch({ payload: e.target.value })}
            rows={4}
            placeholder={
              form.kind === "command"
                ? "df -h"
                : "Check disk usage and report anything above 85%."
            }
          />
        </Field>

        <Field label="Targets">
          <div className="flex flex-wrap gap-1">
            {vpsList.length === 0 && (
              <span className="text-xs text-gray-600">No servers yet.</span>
            )}
            {vpsList.map((v) => (
              <button
                key={v.id}
                type="button"
                onClick={() => toggleTarget(v.id)}
                className={`rounded-full border px-2 py-0.5 text-[10px] ${
                  targets.includes(v.id)
                    ? "border-blue-500 bg-blue-600/30 text-blue-100"
                    : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)]"
                }`}
              >
                {v.name}
              </button>
            ))}
          </div>
        </Field>

        <div className="mt-2">
          <Toggle
            checked={form.enabled}
            onChange={(v) => patch({ enabled: v })}
            label="Enabled"
          />
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            onClick={submit}
            disabled={saving || !form.name.trim() || !form.payload.trim()}
          >
            {saving ? "Saving..." : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function CronSection() {
  const { jobs, load, remove, runNow } = useCronStore();
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<CronJob | null>(null);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const [personas, setPersonas] = useState<Persona[]>([]);

  useEffect(() => {
    load();
    void api.listPersonas().then(setPersonas).catch(() => setPersonas([]));
  }, [load]);

  // An id in the row would tell the reader nothing; a deleted one still has to render
  // as something rather than as a blank.
  const nameOfPersona = (id: string) =>
    personas.find((p) => p.id === id)?.name ?? "a deleted agent";
  const nameOfWorkspace = (id: string) =>
    workspaces.find((w) => w.id === id)?.name ?? "a deleted project";

  return (
    <div>
      <SectionHeader
        title="Cron"
        description="Schedule recurring commands or agent prompts against your servers. Jobs honor each server's safety mode."
        action={
          <Button
            variant="primary"
            onClick={() => {
              setEditing(null);
              setShowForm(true);
            }}
          >
            <PlusIcon size={13} /> New
          </Button>
        }
      />

      {jobs.length === 0 && (
        <Card className="text-center text-xs text-gray-500">No cron jobs yet.</Card>
      )}

      <div className="space-y-2">
        {jobs.map((j) => (
          <Card key={j.id} className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-sm text-gray-200">{j.name}</span>
                <span className="rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-400">
                  {j.schedule}
                </span>
                {!j.enabled && (
                  <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-300">
                    paused
                  </span>
                )}
              </div>
              {/* Who it runs as and where. A schedule that acts on servers under an
                  agent's trust level is a different thing from a shell command, and the
                  row should not make them look alike. */}
              {(j.persona_id || j.workspace_id) && (
                <div className="mt-0.5 truncate font-mono text-[10px] text-gray-400">
                  {j.persona_id ? `as ${nameOfPersona(j.persona_id)}` : "as the main agent"}
                  {j.workspace_id ? ` on ${nameOfWorkspace(j.workspace_id)}` : ""}
                </div>
              )}
              <div className="mt-0.5 truncate text-xs text-gray-500">
                {j.kind} · {j.payload}
                {j.last_run && ` · last: ${j.last_status ?? ""} ${j.last_run}`}
              </div>
            </div>
            <Button onClick={() => runNow(j.id)} title="Run now">
              Run
            </Button>
            <Button
              onClick={() => {
                setEditing(j);
                setShowForm(true);
              }}
            >
              Edit
            </Button>
            <Button
              variant="danger"
              onClick={async () => {
                if (
                  await dialog.confirm({
                    title: "Delete cron job",
                    message: `Delete job "${j.name}"?`,
                    danger: true,
                    confirmText: "Delete",
                  })
                )
                  remove(j.id);
              }}
            >
              <TrashIcon size={14} />
            </Button>
          </Card>
        ))}
      </div>

      {showForm && (
        <CronForm initial={editing} onClose={() => setShowForm(false)} />
      )}
    </div>
  );
}
