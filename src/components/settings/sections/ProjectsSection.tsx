import { useEffect, useState } from "react";
import { useProjectStore } from "../../../stores/projectStore";
import { useVpsStore } from "../../../stores/vpsStore";
import { useCloudStore } from "../../../stores/cloudStore";
import type { InfraProject, InfraProjectInput } from "../../../lib/tauri";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, Select, TextInput } from "../ui";

function emptyProject(): InfraProjectInput {
  return {
    name: "",
    template: "blank",
    backend: "vps",
    default_vps_id: null,
    cloud_account_id: null,
    config_json: "",
    description: "",
  };
}

function ProjectForm({
  initial,
  onClose,
}: {
  initial: InfraProject | null;
  onClose: () => void;
}) {
  const save = useProjectStore((s) => s.save);
  const vpsList = useVpsStore((s) => s.vpsList);
  const loadVps = useVpsStore((s) => s.load);
  const cloudAccounts = useCloudStore((s) => s.accounts);
  const loadCloud = useCloudStore((s) => s.load);

  const [form, setForm] = useState<InfraProjectInput>(
    initial
      ? {
          id: initial.id,
          name: initial.name,
          slug: initial.slug,
          template: initial.template,
          backend: initial.backend ?? "vps",
          default_vps_id: initial.default_vps_id ?? null,
          cloud_account_id: initial.cloud_account_id ?? null,
          config_json: initial.config_json ?? "",
          description: initial.description ?? "",
        }
      : emptyProject(),
  );
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    loadVps();
    loadCloud();
  }, [loadVps, loadCloud]);

  const submit = async () => {
    if (!form.name.trim()) return;
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
      <div className="w-[min(480px,92vw)] rounded-xl border border-[#1f2737] bg-[#0d121b] p-5 shadow-2xl">
        <h3 className="mb-4 text-sm font-semibold text-gray-100">
          {initial ? "Edit project" : "New Terraform project"}
        </h3>
        <div className="space-y-3">
          <Field label="Name">
            <TextInput
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="my-web-stack"
            />
          </Field>
          <Field label="Template">
            <Select
              value={form.template ?? "blank"}
              onChange={(e) => setForm((f) => ({ ...f, template: e.target.value }))}
            >
              <option value="blank">blank</option>
              <option value="vps-web">vps-web (nginx)</option>
              <option value="aws-minimal">aws-minimal (S3 bucket)</option>
              <option value="gcp-minimal">gcp-minimal (GCS bucket)</option>
            </Select>
          </Field>
          <Field label="Backend">
            <Select
              value={form.backend ?? "vps"}
              onChange={(e) => setForm((f) => ({ ...f, backend: e.target.value }))}
            >
              <option value="vps">vps (local state on runner)</option>
              <option value="tfc">tfc (Terraform Cloud remote state)</option>
            </Select>
          </Field>
          <Field label="Cloud account">
            <Select
              value={form.cloud_account_id ?? ""}
              onChange={(e) =>
                setForm((f) => ({
                  ...f,
                  cloud_account_id: e.target.value || null,
                }))
              }
            >
              <option value="">— none —</option>
              {cloudAccounts.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name} ({c.kind})
                </option>
              ))}
            </Select>
          </Field>
          <Field
            label="Config JSON"
            hint='e.g. {"aws_region":"eu-west-1"} or {"tfc_org":"acme","tfc_workspace":"dev"}'
          >
            <TextInput
              value={form.config_json ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, config_json: e.target.value }))}
              placeholder="Optional"
            />
          </Field>
          <Field label="Runner VPS">
            <Select
              value={form.default_vps_id ?? ""}
              onChange={(e) =>
                setForm((f) => ({
                  ...f,
                  default_vps_id: e.target.value || null,
                }))
              }
            >
              <option value="">— select —</option>
              {vpsList.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.name} ({v.host})
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Description">
            <TextInput
              value={form.description ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))}
              placeholder="Optional"
            />
          </Field>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={saving || !form.name.trim()}>
            {saving ? "Saving…" : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function ProjectsSection() {
  const projects = useProjectStore((s) => s.projects);
  const load = useProjectStore((s) => s.load);
  const remove = useProjectStore((s) => s.remove);
  const [editing, setEditing] = useState<InfraProject | null | "new">(null);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="flex h-full flex-col">
      <SectionHeader
        title="Terraform projects"
        description="Local IaC synced to a VPS runner. Link cloud accounts for AWS/GCP/TFC. Agent: meta/ponytail + infra/terraform-* skills."
        action={
          <Button variant="primary" onClick={() => setEditing("new")}>
            <PlusIcon size={13} /> New
          </Button>
        }
      />
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
        {projects.length === 0 ? (
          <p className="text-sm text-gray-500">
            No projects yet. Create one here or ask the agent to deploy infra.
          </p>
        ) : (
          projects.map((p) => (
            <Card key={p.id} className="flex items-start justify-between gap-3 p-3">
              <div>
                <div className="font-medium text-gray-100">{p.name}</div>
                <div className="text-xs text-gray-500">
                  slug: {p.slug} · {p.template} · backend: {p.backend}
                  {p.default_vps_id ? ` · runner: ${p.default_vps_id}` : ""}
                  {p.cloud_account_id ? ` · cloud: ${p.cloud_account_id}` : ""}
                </div>
                {p.description ? (
                  <p className="mt-1 text-xs text-gray-400">{p.description}</p>
                ) : null}
              </div>
              <div className="flex shrink-0 gap-1">
                <Button variant="ghost" onClick={() => setEditing(p)}>
                  Edit
                </Button>
                <Button
                  variant="ghost"
                  className="text-red-400"
                  onClick={() => remove(p.id)}
                >
                  <TrashIcon size={14} />
                </Button>
              </div>
            </Card>
          ))
        )}
      </div>
      {editing === "new" ? (
        <ProjectForm initial={null} onClose={() => setEditing(null)} />
      ) : editing ? (
        <ProjectForm initial={editing} onClose={() => setEditing(null)} />
      ) : null}
    </div>
  );
}
