import { useEffect, useState } from "react";
import { useCloudStore } from "../../../stores/cloudStore";
import type { CloudAccount, CloudAccountInput } from "../../../lib/tauri";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, Select, TextInput } from "../ui";

type CloudKind = "aws" | "gcp" | "tfc";

const KIND_LABELS: Record<CloudKind, string> = {
  aws: "Amazon Web Services",
  gcp: "Google Cloud",
  tfc: "Terraform Cloud",
};

function emptyAccount(): CloudAccountInput {
  return { name: "", kind: "aws", secret: "" };
}

function CloudForm({
  initial,
  onClose,
}: {
  initial: CloudAccount | null;
  onClose: () => void;
}) {
  const save = useCloudStore((s) => s.save);
  const [form, setForm] = useState<CloudAccountInput>(
    initial
      ? {
          id: initial.id,
          name: initial.name,
          kind: initial.kind as CloudKind,
          region: initial.region ?? "",
          project_id: initial.project_id ?? "",
          organization: initial.organization ?? "",
          secret: "",
        }
      : emptyAccount(),
  );
  const [saving, setSaving] = useState(false);
  const kind = form.kind as CloudKind;

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
      <div className="w-[min(520px,92vw)] rounded-xl border border-[#1f2737] bg-[#0d121b] p-5 shadow-2xl">
        <h3 className="mb-4 text-sm font-semibold text-gray-100">
          {initial ? "Edit cloud account" : "Add cloud account"}
        </h3>
        <div className="space-y-3">
          <Field label="Name">
            <TextInput
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
            />
          </Field>
          <Field label="Provider">
            <Select
              value={form.kind}
              onChange={(e) =>
                setForm((f) => ({ ...f, kind: e.target.value as CloudKind }))
              }
            >
              {(Object.keys(KIND_LABELS) as CloudKind[]).map((k) => (
                <option key={k} value={k}>
                  {KIND_LABELS[k]}
                </option>
              ))}
            </Select>
          </Field>
          {kind === "aws" && (
            <Field label="Default region">
              <TextInput
                value={form.region ?? ""}
                onChange={(e) => setForm((f) => ({ ...f, region: e.target.value }))}
                placeholder="us-east-1"
              />
            </Field>
          )}
          {kind === "gcp" && (
            <Field label="GCP project ID">
              <TextInput
                value={form.project_id ?? ""}
                onChange={(e) =>
                  setForm((f) => ({ ...f, project_id: e.target.value }))
                }
              />
            </Field>
          )}
          {kind === "tfc" && (
            <Field label="Organization">
              <TextInput
                value={form.organization ?? ""}
                onChange={(e) =>
                  setForm((f) => ({ ...f, organization: e.target.value }))
                }
              />
            </Field>
          )}
          <Field
            label="Credentials"
            hint={
              kind === "aws"
                ? "Line 1: access key ID, line 2: secret access key (keychain only)"
                : kind === "gcp"
                  ? "Service account JSON"
                  : "Terraform Cloud API token"
            }
          >
            <TextInput
              type="password"
              value={form.secret ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, secret: e.target.value }))}
              placeholder={initial?.has_secret ? "•••••••• (unchanged if empty)" : ""}
            />
          </Field>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} disabled={saving || !form.name.trim()}>
            {saving ? "Saving…" : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function CloudSection() {
  const accounts = useCloudStore((s) => s.accounts);
  const load = useCloudStore((s) => s.load);
  const remove = useCloudStore((s) => s.remove);
  const scanResources = useCloudStore((s) => s.scanResources);
  const [editing, setEditing] = useState<CloudAccount | null | "new">(null);
  const [scanResult, setScanResult] = useState<{ id: string; text: string } | null>(null);
  const [scanning, setScanning] = useState<string | null>(null);

  useEffect(() => {
    load();
  }, [load]);

  const runScan = async (id: string) => {
    setScanning(id);
    setScanResult(null);
    try {
      const text = await scanResources(id);
      setScanResult({ id, text });
    } catch (e) {
      setScanResult({ id, text: String(e) });
    } finally {
      setScanning(null);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <SectionHeader
        title="Cloud accounts"
        description="AWS, GCP, and Terraform Cloud credentials stored in the OS keychain — never in SQLite."
        action={
          <Button variant="primary" onClick={() => setEditing("new")}>
            <PlusIcon size={13} /> Add
          </Button>
        }
      />
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
        {accounts.length === 0 ? (
          <p className="text-sm text-gray-500">No cloud accounts yet.</p>
        ) : (
          accounts.map((a) => (
            <Card key={a.id} className="flex items-start justify-between gap-3 p-3">
              <div>
                <div className="font-medium text-gray-100">{a.name}</div>
                <div className="text-xs text-gray-500">
                  {a.kind.toUpperCase()} · {a.has_secret ? "credentials set" : "no credentials"}
                  {a.region ? ` · ${a.region}` : ""}
                  {a.organization ? ` · org: ${a.organization}` : ""}
                </div>
              </div>
              <div className="flex shrink-0 gap-1">
                {a.kind !== "tfc" && a.has_secret ? (
                  <Button
                    variant="ghost"
                    disabled={scanning === a.id}
                    onClick={() => runScan(a.id)}
                  >
                    {scanning === a.id ? "Scanning…" : "Scan"}
                  </Button>
                ) : null}
                <Button variant="ghost" onClick={() => setEditing(a)}>
                  Edit
                </Button>
                <Button
                  variant="ghost"
                  className="text-red-400"
                  onClick={() => remove(a.id)}
                >
                  <TrashIcon size={14} />
                </Button>
              </div>
            </Card>
          ))
        )}
        {scanResult ? (
          <Card className="p-3">
            <div className="mb-1 text-xs font-medium text-gray-300">Resource scan</div>
            <pre className="max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-gray-400">
              {scanResult.text}
            </pre>
          </Card>
        ) : null}
      </div>
      {editing === "new" ? (
        <CloudForm initial={null} onClose={() => setEditing(null)} />
      ) : editing ? (
        <CloudForm initial={editing} onClose={() => setEditing(null)} />
      ) : null}
    </div>
  );
}
