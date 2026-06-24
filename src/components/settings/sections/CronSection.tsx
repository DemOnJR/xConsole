import { useEffect, useState } from "react";
import { useCronStore } from "../../../stores/cronStore";
import { useVpsStore } from "../../../stores/vpsStore";
import { dialog } from "../../../stores/dialogStore";
import type { CronJob, CronJobInput } from "../../../lib/tauri";
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
        }
      : emptyJob(),
  );
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    loadVps();
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

  useEffect(() => {
    load();
  }, [load]);

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
