import { useEffect, useState } from "react";
import {
  api,
  type KnownHost,
  type LockStatus,
  type ScannerStatus,
} from "../../../lib/tauri";
import { dialog } from "../../../stores/dialogStore";
import { Button, Card, SectionHeader } from "../ui";
import { TrashIcon } from "../../icons";

const inputCls =
  "w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-3 py-2 text-sm text-gray-100 outline-none focus:border-[var(--accent)]";

/** App lock / at-rest DB encryption management (set up, change password, disable, export). */
function AppLockCard() {
  const [status, setStatus] = useState<LockStatus | null>(null);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [remember, setRemember] = useState(true);
  const [ack, setAck] = useState(false);
  const [oldPw, setOldPw] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");

  const refresh = () => api.lockStatus().then(setStatus).catch(() => {});
  useEffect(() => {
    refresh();
  }, []);
  if (!status) return null;

  const run = async (fn: () => Promise<string | void>, working: string) => {
    setBusy(true);
    setMsg(working);
    try {
      const r = await fn();
      setMsg(typeof r === "string" ? r : "Done.");
      setPw("");
      setPw2("");
      setOldPw("");
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
    if (pw !== pw2) return setMsg("The two passwords don't match.");
    if (!ack) return setMsg("Please confirm you understand there is no recovery.");
    void run(async () => {
      await api.setupLock(pw, remember);
      return "App lock enabled — your database is now encrypted at rest.";
    }, "Encrypting your data…");
  };

  return (
    <Card className="mb-3">
      <div className="mb-2 text-sm font-medium text-gray-200">
        🔒 App lock &amp; database encryption
      </div>

      {!status.enabled ? (
        <>
          <p className="mb-3 text-xs text-gray-400">
            Encrypt your database (chats, servers, workspaces, settings) at rest with a master
            password, so a stolen <code>.db</code> file is useless without it.
          </p>
          <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5 text-[11px] text-amber-200">
            ⚠ There is <b>no password reset and no recovery</b>. If you forget this password and
            don't have this device remembered, your data is gone <b>forever</b>. Consider exporting
            an unencrypted backup first and storing it safely.
          </div>
          <div className="mt-3 space-y-2">
            <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Master password" className={inputCls} />
            <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} placeholder="Confirm master password" className={inputCls} />
            <label className="flex items-center gap-2 text-xs text-gray-300">
              <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
              Remember on this device (unlock automatically; otherwise you'll type it each launch)
            </label>
            <label className="flex items-center gap-2 text-xs text-gray-300">
              <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
              I understand there is no way to recover my data if I forget this password.
            </label>
            <div className="flex gap-2">
              <Button variant="primary" onClick={enable} disabled={busy}>
                {busy ? "Working…" : "Enable app lock"}
              </Button>
              <Button onClick={() => void run(() => api.exportUnencryptedBackup(), "Exporting…")} disabled={busy}>
                Export unencrypted backup
              </Button>
            </div>
          </div>
        </>
      ) : (
        <>
          <p className="mb-3 text-xs text-gray-400">
            Your database is <b>encrypted at rest</b>.{" "}
            {status.remembered ? "This device is remembered (silent unlock)." : "This device is not remembered — you'll enter your password each launch."}
          </p>
          <div className="space-y-3">
            <div>
              <div className="mb-1 text-[11px] uppercase tracking-wide text-gray-500">Change master password</div>
              <div className="space-y-2">
                <input type="password" value={oldPw} onChange={(e) => setOldPw(e.target.value)} placeholder="Current password" className={inputCls} />
                <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="New password" className={inputCls} />
                <Button onClick={() => void run(() => api.changePassword(oldPw, pw), "Updating…")} disabled={busy || !oldPw || !pw}>
                  Change password
                </Button>
              </div>
            </div>
            <div className="flex flex-wrap gap-2 border-t border-[var(--border)] pt-3">
              <Button onClick={() => void run(() => api.exportUnencryptedBackup(), "Exporting…")} disabled={busy}>
                Export unencrypted backup
              </Button>
              <Button
                onClick={async () => {
                  if (await dialog.confirm({ title: "Forget this device", message: "After this you'll need your master password to open xConsole on this device. If you've forgotten it, you'll be locked out permanently. Continue?", danger: true, confirmText: "Forget device" }))
                    void run(() => api.forgetDevice(), "Forgetting…");
                }}
                disabled={busy}
              >
                Forget this device
              </Button>
            </div>
            <div className="border-t border-[var(--border)] pt-3">
              <div className="mb-1 text-[11px] uppercase tracking-wide text-gray-500">Turn off app lock</div>
              <div className="flex items-end gap-2">
                <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} placeholder="Confirm with your password" className={inputCls} />
                <Button variant="danger" onClick={() => void run(() => api.disableLock(pw2), "Disabling…")} disabled={busy || !pw2}>
                  Disable
                </Button>
              </div>
            </div>
          </div>
        </>
      )}

      {msg && <div className="mt-3 text-[11px] text-gray-400">{msg}</div>}
    </Card>
  );
}

/** Skill security scanner status + one-click install of NVIDIA SkillSpector. */
function SkillScannerCard() {
  const [status, setStatus] = useState<ScannerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");

  const refresh = () => api.skillScannerStatus().then(setStatus).catch(() => {});
  useEffect(() => {
    refresh();
  }, []);

  const install = async () => {
    setBusy(true);
    setMsg("Installing SkillSpector (this can take a minute)…");
    try {
      setMsg(await api.installSkillScanner());
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      refresh();
    }
  };

  const installed = status?.installed ?? false;

  return (
    <Card className="mb-3">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm text-gray-200">Skill security scanner</div>
          <div className="mt-0.5 text-xs text-gray-500">
            Skills (including ones the agent researches) are scanned before they're
            saved or installed. NVIDIA SkillSpector is the strong static analyzer;
            without it a built-in heuristic is used as a fallback.
          </div>
        </div>
        <div className="shrink-0 text-right">
          {installed ? (
            <span className="rounded-full bg-emerald-500/15 px-2 py-1 text-[11px] text-emerald-400">
              SkillSpector active
            </span>
          ) : (
            <span className="rounded-full bg-amber-500/15 px-2 py-1 text-[11px] text-amber-400">
              Built-in heuristic
            </span>
          )}
        </div>
      </div>

      <div className="mt-2 font-mono text-[11px] text-gray-500">
        {installed
          ? status?.version ?? "SkillSpector installed"
          : status?.uv_available
            ? "SkillSpector not installed (uv is available)"
            : "SkillSpector not installed — uv is required to install it"}
      </div>

      {!installed && (
        <div className="mt-3 flex items-center gap-2">
          <Button
            onClick={() => void install()}
            disabled={busy || !(status?.uv_available ?? false)}
            title={status?.uv_available ? "Install SkillSpector via uv" : "Install uv first"}
          >
            {busy ? "Installing…" : "Install SkillSpector"}
          </Button>
          {!status?.uv_available && (
            <span className="text-[11px] text-gray-500">
              Install uv from docs.astral.sh/uv first.
            </span>
          )}
        </div>
      )}

      {msg && <div className="mt-2 text-[11px] text-gray-400">{msg}</div>}
    </Card>
  );
}

export function SecuritySection() {
  const [hosts, setHosts] = useState<KnownHost[]>([]);

  const load = () => api.listKnownHosts().then(setHosts);
  useEffect(() => {
    load();
  }, []);

  const forget = async (h: KnownHost) => {
    if (
      !(await dialog.confirm({
        title: "Forget host key",
        message: `Forget pinned key for ${h.host}:${h.port}?`,
        danger: true,
        confirmText: "Forget",
      }))
    )
      return;
    await api.forgetHostKey(h.host, h.port);
    load();
  };

  return (
    <div>
      <SectionHeader
        title="Security"
        description="App lock (at-rest encryption) and pinned SSH host keys (trust-on-first-use)."
      />

      <AppLockCard />
      <SkillScannerCard />

      <div className="mb-2 mt-4 text-[11px] uppercase tracking-wide text-gray-500">
        Pinned SSH host keys
      </div>

      {hosts.length === 0 && (
        <Card className="text-center text-xs text-gray-500">
          No pinned hosts yet.
        </Card>
      )}

      <div className="space-y-2">
        {hosts.map((h) => (
          <Card key={`${h.host}:${h.port}`} className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm text-gray-200">
                {h.host}:{h.port}
              </div>
              <div className="truncate font-mono text-[11px] text-gray-500">
                {h.key_type} · {h.fingerprint}
              </div>
            </div>
            <Button variant="danger" onClick={() => forget(h)} title="Forget key">
              <TrashIcon size={14} />
            </Button>
          </Card>
        ))}
      </div>
    </div>
  );
}
