import { useEffect, useState } from "react";
import { api, type KnownHost, type LockStatus } from "../../../lib/tauri";
import { dialog } from "../../../stores/dialogStore";
import { Button, SectionHeader, Toggle, SettingsGroup, SettingsRow, TextInput } from "../ui";
import { TrashIcon, ShieldIcon } from "../../icons";
import { usePrivacyStore } from "../../../stores/privacyStore";
import { useMaskHost } from "../../../lib/privacy";

/** App lock / at-rest DB encryption management (set up, change password, disable, export). */
function AppLockCard() {
  const [status, setStatus] = useState<LockStatus | null>(null);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [remember, setRemember] = useState(false);
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");

  const [autoLock, setAutoLock] = useState<number | null>(null);

  const refresh = () => api.lockStatus().then(setStatus).catch(() => {});
  useEffect(() => {
    refresh();
    api.getAutoLockMinutes().then(setAutoLock).catch(() => {});
  }, []);
  if (!status) return null;

  const run = async (fn: () => Promise<string | void>, working: string) => {
    setBusy(true);
    setMsg(working);
    try {
      const r = await fn();
      setMsg(typeof r === "string" ? r : "Operation completed.");
      setPw("");
      setPw2("");
      setAck(false);
      refresh();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  const enable = () => {
    if (pw.length < 12) return setMsg("Master password must be at least 12 characters.");
    if (pw !== pw2) return setMsg("The two passwords do not match.");
    if (!ack) return setMsg("Please confirm that you understand there is no password recovery.");
    void run(async () => {
      await api.setupLock(pw, remember);
      return "App lock enabled — your database is now encrypted at rest.";
    }, "Encrypting data…");
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <div className="flex h-6 w-6 items-center justify-center rounded bg-cyan-500/10 text-cyan-400">
            <ShieldIcon size={14} />
          </div>
          <span className="text-xs font-semibold text-gray-200 uppercase tracking-wide">
            App Lock &amp; Database Encryption
          </span>
        </div>
        <span
          className={`rounded px-2 py-0.5 text-[10px] font-mono font-medium ${
            status.enabled
              ? "bg-emerald-500/15 text-emerald-400 border border-emerald-500/30"
              : "bg-zinc-800 text-zinc-400 border border-zinc-700"
          }`}
        >
          {status.enabled ? "Encrypted at Rest" : "Unencrypted"}
        </span>
      </div>

      {!status.enabled ? (
        <div className="space-y-3 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-4">
          <p className="text-xs text-gray-300 leading-relaxed">
            Encrypt your database (chats, servers, workspaces, credentials) at rest with a master
            password. Without it, a stolen SQLite <code>.db</code> file cannot be read.
          </p>

          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-200/90 leading-relaxed">
            <span className="font-semibold text-amber-300">Important:</span> There is no password reset
            mechanism. If you forget this password and have not exported a backup, your local data cannot be recovered.
          </div>

          <div className="space-y-2.5 pt-1">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
              <TextInput
                type="password"
                value={pw}
                onChange={(e) => setPw(e.target.value)}
                placeholder="Master password (min 12 chars)"
              />
              <TextInput
                type="password"
                value={pw2}
                onChange={(e) => setPw2(e.target.value)}
                placeholder="Confirm master password"
              />
            </div>

            <div className="space-y-1.5 pt-1">
              <label className="flex items-center gap-2 text-xs text-gray-300 cursor-pointer">
                <input
                  type="checkbox"
                  checked={remember}
                  onChange={(e) => setRemember(e.target.checked)}
                  className="rounded border-zinc-700 bg-zinc-900 text-cyan-500"
                />
                <span>Remember on this device (unlock automatically on app launch)</span>
              </label>
              <label className="flex items-center gap-2 text-xs text-gray-300 cursor-pointer">
                <input
                  type="checkbox"
                  checked={ack}
                  onChange={(e) => setAck(e.target.checked)}
                  className="rounded border-zinc-700 bg-zinc-900 text-cyan-500"
                />
                <span>I understand there is no recovery if I lose this password</span>
              </label>
            </div>

            <div className="flex items-center gap-2 pt-2">
              <Button variant="primary" onClick={enable} disabled={busy}>
                {busy ? "Encrypting…" : "Enable App Lock"}
              </Button>
              <Button
                onClick={() => void run(() => api.exportUnencryptedBackup(""), "Exporting backup…")}
                disabled={busy}
              >
                Export Plaintext Backup
              </Button>
            </div>
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          <SettingsRow
            label="Encrypt Saved Credentials"
            description="Encrypts SSH passwords, private keys, and API tokens with the master password before storing in OS credential manager."
          >
            <Toggle
              checked={status.secrets_encrypted}
              onChange={(v) =>
                void run(async () => {
                  const n = await api.setSecretEncryption(v);
                  return v
                    ? `Encrypted ${n} saved credential(s).`
                    : `Decrypted ${n} saved credential(s).`;
                }, v ? "Encrypting secrets…" : "Decrypting secrets…")
              }
            />
          </SettingsRow>

          <SettingsRow
            label="Auto-Lock When Idle"
            description="Automatically locks the workspace and closes active SSH sessions after inactivity."
          >
            <select
              value={autoLock ?? 60}
              onChange={(e) => {
                const m = Number(e.target.value);
                setAutoLock(m);
                void api.setAutoLockMinutes(m).catch((err) => setMsg(String(err)));
              }}
              className="rounded-md border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1 text-xs text-gray-200 outline-none"
            >
              <option value={1}>1 minute</option>
              <option value={5}>5 minutes</option>
              <option value={15}>15 minutes</option>
              <option value={30}>30 minutes</option>
              <option value={60}>1 hour</option>
              <option value={0}>Never</option>
            </select>
          </SettingsRow>
        </div>
      )}

      {msg && <p className="text-xs font-mono text-cyan-400 mt-2">{msg}</p>}
    </div>
  );
}

export function SecuritySection() {
  const maskIps = usePrivacyStore((s) => s.maskIps);
  const setMaskIps = usePrivacyStore((s) => s.setMaskIps);
  const maskHost = useMaskHost();
  const [knownHosts, setKnownHosts] = useState<KnownHost[]>([]);
  const [loadingHosts, setLoadingHosts] = useState(false);

  const loadKnownHosts = () => {
    setLoadingHosts(true);
    api
      .listKnownHosts()
      .then(setKnownHosts)
      .catch(() => {})
      .finally(() => setLoadingHosts(false));
  };

  useEffect(() => {
    loadKnownHosts();
  }, []);

  const removeHost = async (h: KnownHost) => {
    const ok = await dialog.confirm({
      title: "Remove Known Host",
      message: `Remove SSH host fingerprint for "${maskHost(h.host)}:${h.port}"? You will be prompted to re-verify its key on next connection.`,
      confirmText: "Remove",
      danger: true,
    });
    if (ok) {
      await api.forgetHostKey(h.host, h.port);
      loadKnownHosts();
    }
  };

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Security & Privacy"
        description="Database encryption at rest, privacy redactions in terminal outputs, and SSH host key verification."
      />

      <AppLockCard />

      <SettingsGroup
        title="Privacy & Data Masking"
        description="Prevent sensitive hostnames, IP addresses, and tokens from leaking into AI agent prompts or screen recordings."
        className="pt-4 border-t border-[var(--border)]"
      >
        <SettingsRow
          label="Mask Remote Hostnames & IPs"
          description="Replaces real hostnames and IP addresses with synthetic aliases in agent logs and AI model contexts."
        >
          <Toggle checked={maskIps} onChange={setMaskIps} />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup
        title="Verified SSH Known Hosts"
        description="Public key fingerprints verified during previous SSH handshakes."
        className="pt-4 border-t border-[var(--border)]"
      >
        {knownHosts.length === 0 ? (
          <p className="text-xs text-gray-500 italic">
            {loadingHosts ? "Loading host keys…" : "No verified host fingerprints saved."}
          </p>
        ) : (
          <div className="max-h-48 overflow-y-auto rounded-lg border border-[var(--border)] bg-[var(--surface)] divide-y divide-[var(--border)]">
            {knownHosts.map((kh) => (
              <div
                key={`${kh.host}:${kh.port}`}
                className="flex items-center justify-between p-2.5 text-xs"
              >
                <div className="min-w-0 flex-1 pr-3">
                  <div className="font-mono text-gray-200 truncate">
                    {maskHost(kh.host)}:{kh.port}
                  </div>
                  <div className="font-mono text-[10px] text-gray-500 truncate">
                    {kh.key_type} · {kh.fingerprint}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => removeHost(kh)}
                  className="rounded p-1 text-gray-500 hover:bg-red-500/10 hover:text-red-400 transition"
                  title="Remove known host"
                >
                  <TrashIcon size={13} />
                </button>
              </div>
            ))}
          </div>
        )}
      </SettingsGroup>
    </div>
  );
}
