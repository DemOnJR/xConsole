import { useState } from "react";
import { useVpsStore } from "../stores/vpsStore";
import type { AuthType, Vps, VpsInput } from "../lib/tauri";

const FIELD =
  "w-full rounded-md border border-[#1f2737] bg-[#0b0f17] px-2.5 py-1.5 text-sm text-gray-200 outline-none focus:border-blue-500";
const LABEL = "mb-1 block text-xs font-medium text-gray-400";

export function VpsForm({
  initial,
  onClose,
}: {
  initial?: Vps | null;
  onClose: () => void;
}) {
  const save = useVpsStore((s) => s.save);
  const [form, setForm] = useState<VpsInput>(() =>
    initial
      ? {
          id: initial.id,
          name: initial.name,
          host: initial.host,
          port: initial.port,
          username: initial.username,
          auth_type: initial.auth_type,
          key_path: initial.key_path ?? "",
          tags: initial.tags ?? "",
        }
      : {
          name: "",
          host: "",
          port: 22,
          username: "root",
          auth_type: "key",
          key_path: "",
          tags: "",
        },
  );
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const set = <K extends keyof VpsInput>(k: K, v: VpsInput[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  const submit = async () => {
    if (!form.name || !form.host || !form.username) {
      setErr("Name, host and username are required.");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await save({ ...form, secret: secret || undefined });
      onClose();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onMouseDown={onClose}
    >
      <div
        className="w-[420px] rounded-xl border border-[#1f2737] bg-[#11161f] p-5 shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 className="mb-4 text-base font-semibold text-gray-100">
          {initial ? "Edit VPS" : "Add VPS"}
        </h2>

        <div className="space-y-3">
          <div>
            <label className={LABEL}>Name</label>
            <input
              className={FIELD}
              value={form.name}
              onChange={(e) => set("name", e.target.value)}
              placeholder="prod-web-1"
            />
          </div>

          <div className="flex gap-2">
            <div className="flex-1">
              <label className={LABEL}>Host</label>
              <input
                className={FIELD}
                value={form.host}
                onChange={(e) => set("host", e.target.value)}
                placeholder="203.0.113.10"
              />
            </div>
            <div className="w-24">
              <label className={LABEL}>Port</label>
              <input
                className={FIELD}
                type="number"
                value={form.port}
                onChange={(e) => set("port", Number(e.target.value) || 22)}
              />
            </div>
          </div>

          <div>
            <label className={LABEL}>Username</label>
            <input
              className={FIELD}
              value={form.username}
              onChange={(e) => set("username", e.target.value)}
            />
          </div>

          <div>
            <label className={LABEL}>Authentication</label>
            <select
              className={FIELD}
              value={form.auth_type}
              onChange={(e) => set("auth_type", e.target.value as AuthType)}
            >
              <option value="key">Private key (file)</option>
              <option value="password">Password</option>
              <option value="agent">ssh-agent</option>
            </select>
          </div>

          {form.auth_type === "key" && (
            <>
              <div>
                <label className={LABEL}>Private key path</label>
                <input
                  className={FIELD}
                  value={form.key_path ?? ""}
                  onChange={(e) => set("key_path", e.target.value)}
                  placeholder="C:\\Users\\you\\.ssh\\id_ed25519"
                />
              </div>
              <div>
                <label className={LABEL}>
                  Key passphrase{" "}
                  <span className="text-gray-600">(optional, kept in OS keychain)</span>
                </label>
                <input
                  className={FIELD}
                  type="password"
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                  placeholder={initial ? "(unchanged)" : ""}
                />
              </div>
            </>
          )}

          {form.auth_type === "password" && (
            <div>
              <label className={LABEL}>
                Password{" "}
                <span className="text-gray-600">(kept in OS keychain)</span>
              </label>
              <input
                className={FIELD}
                type="password"
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
                placeholder={initial ? "(unchanged)" : ""}
              />
            </div>
          )}

          <div>
            <label className={LABEL}>Tags (comma separated)</label>
            <input
              className={FIELD}
              value={form.tags ?? ""}
              onChange={(e) => set("tags", e.target.value)}
              placeholder="prod, eu-west"
            />
          </div>

          {err && <p className="text-xs text-red-400">{err}</p>}
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <button
            className="rounded-md px-3 py-1.5 text-sm text-gray-300 hover:bg-[#1f2737]"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            disabled={busy}
            className="rounded-md bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-500 disabled:opacity-50"
            onClick={submit}
          >
            {busy ? "Saving..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
