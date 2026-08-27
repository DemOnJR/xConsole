import { useEffect, useState } from "react";
import { useCloudStore } from "../../../stores/cloudStore";
import { api, type CloudAccount, type CloudAccountInput } from "../../../lib/tauri";
import { openUrl } from "@tauri-apps/plugin-opener";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, Select, TextInput } from "../ui";

type CloudKind = "aws" | "gcp" | "tfc" | "cloudflare";

const KIND_LABELS: Record<CloudKind, string> = {
  cloudflare: "Cloudflare (Tunnels, DNS, Security)",
  aws: "Amazon Web Services",
  gcp: "Google Cloud",
  tfc: "Terraform Cloud",
};

function emptyAccount(): CloudAccountInput {
  return { name: "", kind: "cloudflare", secret: "" };
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
      <div className="w-[min(520px,92vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
        <h3 className="mb-4 text-sm font-semibold text-gray-100">
          {initial ? "Edit cloud account" : "Add cloud account"}
        </h3>
        <div className="space-y-3">
          <Field label="Name">
            <TextInput
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="e.g. Cloudflare Main"
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
          {kind === "cloudflare" && (
            <>
              <Field
                label="Account ID"
                hint="Găsit în Cloudflare Dashboard (Overview &rarr; Account ID în bara laterală)"
              >
                <TextInput
                  value={form.project_id ?? ""}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, project_id: e.target.value }))
                  }
                  placeholder="e.g. 1a2b3c4d5e6f..."
                />
              </Field>
              <Field
                label="Default Zone ID (opțional)"
                hint="ID-ul domeniului principal pentru DNS și WAF"
              >
                <TextInput
                  value={form.region ?? ""}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, region: e.target.value }))
                  }
                  placeholder="e.g. 9z8y7x6w..."
                />
              </Field>
            </>
          )}
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
          {kind === "cloudflare" && (
            <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3 text-[11px] space-y-1.5 text-gray-300">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-white">📖 Permisiuni necesare pentru Token:</span>
                <button
                  type="button"
                  className="text-xs text-[#f48120] hover:underline"
                  onClick={() => openUrl("https://dash.cloudflare.com/profile/api-tokens")}
                >
                  Deschide Cloudflare API Tokens ↗
                </button>
              </div>
              <p className="text-gray-400">
                Apasă <strong>+ Create Token</strong> &rarr; <strong>Custom token (Get started)</strong> și adaugă:
              </p>
              <ul className="list-disc list-inside text-gray-300 space-y-0.5 font-mono text-[10px]">
                <li>Account &rarr; Cloudflare Tunnel (Edit)</li>
                <li>Zone &rarr; DNS (Edit)</li>
                <li>Zone &rarr; Zone Settings (Edit)</li>
                <li>Zone &rarr; Zone (Read)</li>
              </ul>
              <p className="text-gray-400 text-[10px] pt-1 border-t border-white/5">
                <em>Sau folosește <strong>Global API Key</strong> (Profil &rarr; API Tokens &rarr; Global API Key &rarr; View).</em>
              </p>
            </div>
          )}
          <Field
            label="Credentials"
            hint={
              kind === "cloudflare"
                ? "API Token sau Global API Key (salvat securizat în Keychain)"
                : kind === "aws"
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
              placeholder={initial?.has_secret ? "•••••••• (unchanged if empty)" : "API Token / Key"}
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
  const [loggingInCf, setLoggingInCf] = useState(false);

  useEffect(() => {
    load();
  }, [load]);

  const handle1ClickCloudflareLogin = async () => {
    setLoggingInCf(true);
    try {
      const authUrl = await api.startCloudflareOAuthLogin();
      await openUrl(authUrl);
      // Poll accounts in background for 60 seconds
      let attempts = 0;
      const interval = setInterval(async () => {
        attempts++;
        await load();
        if (attempts > 30) {
          clearInterval(interval);
          setLoggingInCf(false);
        }
      }, 2000);
    } catch (e) {
      alert(`Eroare la pornirea conectării Cloudflare: ${e}`);
      setLoggingInCf(false);
    }
  };

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
        description="Cloudflare, AWS, GCP și Terraform Cloud stocate securizat în OS keychain — niciodată în SQLite."
        action={
          <div className="flex items-center gap-2">
            <Button
              variant="primary"
              className="bg-[#f48120] hover:bg-[#e06d0e] text-white border-none shadow-sm"
              disabled={loggingInCf}
              onClick={handle1ClickCloudflareLogin}
            >
              {loggingInCf ? "Așteptare autorizare…" : "☁️ Sign in with Cloudflare (1-Click)"}
            </Button>
            <Button variant="ghost" onClick={() => setEditing("new")}>
              <PlusIcon size={13} /> Add manual
            </Button>
          </div>
        }
      />
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
        {accounts.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-[var(--border)] p-8 text-center">
            <div className="text-3xl mb-2">☁️</div>
            <h4 className="text-sm font-semibold text-gray-200 mb-1">Niciun cont cloud configurat</h4>
            <p className="text-xs text-gray-400 max-w-sm mb-4">
              Conectează-te cu 1-Click la Cloudflare pentru a gestiona tunele Zero Trust, înregistrări DNS și setări de securitate WAF.
            </p>
            <Button
              variant="primary"
              className="bg-[#f48120] hover:bg-[#e06d0e] text-white border-none"
              disabled={loggingInCf}
              onClick={handle1ClickCloudflareLogin}
            >
              {loggingInCf ? "Așteptare conectare în browser…" : "☁️ Conectare 1-Click cu Cloudflare"}
            </Button>
          </div>
        ) : (
          accounts.map((a) => (
            <Card key={a.id} className="flex items-start justify-between gap-3 p-3">
              <div>
                <div className="font-medium text-gray-100 flex items-center gap-2">
                  {a.kind === "cloudflare" && <span className="text-xs text-[#f48120]">☁️</span>}
                  {a.name}
                </div>
                <div className="text-xs text-gray-500">
                  {a.kind.toUpperCase()} · {a.has_secret ? "cheie salvată în keychain" : "fără credențiale"}
                  {a.project_id ? ` · account: ${a.project_id.slice(0, 10)}…` : ""}
                  {a.organization ? ` · org: ${a.organization}` : ""}
                </div>
              </div>
              <div className="flex shrink-0 gap-1">
                {a.has_secret ? (
                  <Button
                    variant="ghost"
                    disabled={scanning === a.id}
                    onClick={() => runScan(a.id)}
                  >
                    {scanning === a.id ? "Scanning…" : "Scan / Status"}
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
