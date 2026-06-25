import { useState } from "react";
import { useLockStore } from "../../stores/lockStore";

/** Full-screen splash while the lock status is being read. */
export function SplashScreen() {
  return (
    <div className="flex h-screen w-screen items-center justify-center bg-[var(--bg)]">
      <div className="text-sm text-gray-500">Starting xConsole…</div>
    </div>
  );
}

/** Full-screen unlock gate. Nothing else mounts (no DB-touching effects run) until the
 *  master password decrypts the database. */
export function UnlockScreen() {
  const unlock = useLockStore((s) => s.unlock);
  const error = useLockStore((s) => s.error);
  const busy = useLockStore((s) => s.busy);
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(true);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!password || busy) return;
    void unlock(password, remember).then((ok) => {
      if (ok) setPassword("");
    });
  };

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-[var(--bg)]">
      <form
        onSubmit={submit}
        className="w-[360px] rounded-2xl border border-[var(--border)] bg-[var(--surface-2)] p-6 shadow-2xl"
      >
        <div className="mb-1 text-lg font-semibold text-gray-100">🔒 xConsole is locked</div>
        <p className="mb-4 text-xs text-gray-400">
          Enter your master password to decrypt your servers, chats, and settings.
        </p>

        <input
          type="password"
          autoFocus
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="Master password"
          className="w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-3 py-2 text-sm text-gray-100 outline-none focus:border-[var(--accent)]"
        />

        <label className="mt-3 flex items-center gap-2 text-xs text-gray-300">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
          />
          Remember on this device (unlock automatically next time)
        </label>

        {error && (
          <div className="mt-3 rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2 text-xs text-red-300">
            {error}
          </div>
        )}

        <button
          type="submit"
          disabled={busy || !password}
          className="mt-4 w-full rounded-md bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? "Unlocking…" : "Unlock"}
        </button>

        <p className="mt-3 text-[11px] text-gray-500">
          There is no password reset. Without this password (or a remembered device) your data
          can't be recovered.
        </p>
      </form>
    </div>
  );
}
