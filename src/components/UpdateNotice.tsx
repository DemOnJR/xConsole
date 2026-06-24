import { useEffect } from "react";
import { useUpdateStore } from "../stores/updateStore";

// A non-blocking bottom-right card. Appears when an update is available (Update now
// / Later), shows download progress, and briefly confirms "up to date" / errors
// after a manual check. Never interrupts the user's work.
export function UpdateNotice() {
  const status = useUpdateStore((s) => s.status);
  const version = useUpdateStore((s) => s.version);
  const notes = useUpdateStore((s) => s.notes);
  const progress = useUpdateStore((s) => s.progress);
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
      status === "downloading" ||
      status === "installing" ||
      status === "uptodate" ||
      status === "error");
  if (!show) return null;

  return (
    <div className="fixed bottom-4 right-4 z-[60] w-[320px] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-3.5 shadow-2xl">
      {status === "uptodate" ? (
        <div className="flex items-center gap-2 text-sm text-gray-200">
          <span className="text-green-400">✓</span> You're on the latest version.
        </div>
      ) : status === "error" ? (
        <div className="text-sm">
          <div className="font-medium text-red-300">Update check failed</div>
          <div className="mt-1 max-h-24 overflow-y-auto text-xs text-gray-400">{error}</div>
        </div>
      ) : (
        <>
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold text-gray-100">
                Update available{version ? ` — v${version}` : ""}
              </div>
              {notes && status === "available" && (
                <div className="mt-1 max-h-28 overflow-y-auto whitespace-pre-wrap text-xs text-gray-400">
                  {notes}
                </div>
              )}
            </div>
            {status === "available" && (
              <button
                onClick={dismiss}
                data-tooltip="Dismiss"
                className="rounded px-1.5 text-gray-500 hover:text-gray-300"
              >
                ✕
              </button>
            )}
          </div>

          {(status === "downloading" || status === "installing") && (
            <div className="mt-3">
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--border)]">
                <div
                  className="h-full bg-blue-500 transition-[width]"
                  style={{ width: `${Math.round(progress * 100)}%` }}
                />
              </div>
              <div className="mt-1.5 text-xs text-gray-400">
                {status === "installing"
                  ? "Installing — the app will restart…"
                  : `Downloading… ${Math.round(progress * 100)}%`}
              </div>
            </div>
          )}

          {status === "available" && (
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
                Update &amp; restart
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
