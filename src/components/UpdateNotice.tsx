import { useEffect } from "react";
import { useUpdateStore } from "../stores/updateStore";

// A non-blocking bottom-right card. Appears when an update is available (Update now /
// Later), and switches to a "rebuilding" note once accepted (the installer shows its own
// progress window). Briefly confirms "up to date" / errors after a manual check.
export function UpdateNotice() {
  const status = useUpdateStore((s) => s.status);
  const version = useUpdateStore((s) => s.version);
  const notes = useUpdateStore((s) => s.notes);
  const channel = useUpdateStore((s) => s.channel);
  const note = useUpdateStore((s) => s.note);
  const error = useUpdateStore((s) => s.error);
  const dismissed = useUpdateStore((s) => s.dismissed);
  const install = useUpdateStore((s) => s.install);
  const dismiss = useUpdateStore((s) => s.dismiss);

  // Auto-dismiss the transient "up to date" / error toasts.
  useEffect(() => {
    if (status === "uptodate" || status === "error") {
      const t = setTimeout(() => dismiss(), 5000);
      return () => clearTimeout(t);
    }
  }, [status, dismiss]);

  const show =
    !dismissed &&
    (status === "available" ||
      status === "updating" ||
      status === "uptodate" ||
      status === "error");
  if (!show) return null;

  return (
    <div className="fixed bottom-4 right-4 z-[60] w-[340px] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-3.5 shadow-2xl">
      {status === "uptodate" ? (
        <div className="flex items-center gap-2 text-sm text-gray-200">
          <span className="text-green-400">✓</span> You&apos;re on the latest{" "}
          <span className="font-medium">{channel === "dev" ? "dev" : "stable"}</span> build.
        </div>
      ) : status === "error" ? (
        <div className="text-sm">
          <div className="font-medium text-red-300">Update failed</div>
          <div className="mt-1 max-h-24 overflow-y-auto text-xs text-gray-400">{error}</div>
        </div>
      ) : status === "updating" ? (
        <div className="text-sm">
          <div className="font-semibold text-gray-100">
            Updating xConsole ({channel === "dev" ? "dev" : "stable"})…
          </div>
          <div className="mt-1.5 text-xs text-gray-400">
            The installer is rebuilding from the{" "}
            <b>{channel}</b> branch in its own window — this can take{" "}
            <b>10–20 minutes</b>. Your chats, workspaces, agent memory, and settings are
            backed up and stay intact. The app will close and reopen when it&apos;s done.
          </div>
        </div>
      ) : (
        <>
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold text-gray-100">
                Update available{version ? ` — ${version}` : ""}{" "}
                <span className="text-xs font-normal text-[var(--accent)]">
                  ({channel === "dev" ? "dev" : "stable"})
                </span>
              </div>
              {note && (
                <div className="mt-1 text-xs text-amber-300/90">{note}</div>
              )}
              {notes && (
                <div className="mt-1 max-h-28 overflow-y-auto whitespace-pre-wrap text-xs text-gray-400">
                  {notes}
                </div>
              )}
            </div>
            <button
              onClick={dismiss}
              data-tooltip="Dismiss"
              className="rounded px-1.5 text-gray-500 hover:text-gray-300"
            >
              ✕
            </button>
          </div>

          <div className="mt-3 flex justify-end gap-2">
            <button
              onClick={dismiss}
              className="rounded-md px-2.5 py-1 text-xs text-gray-300 hover:bg-[var(--border)]"
            >
              Later
            </button>
            <button
              onClick={() => void install()}
              className="rounded-md bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-500"
            >
              Update now
            </button>
          </div>
        </>
      )}
    </div>
  );
}
