import { useEffect } from "react";
import { createPortal } from "react-dom";
import { useTransferStore, CONCURRENCY_CHOICES } from "../stores/transferStore";
import { formatBytes, formatDuration, formatRate, percent } from "../lib/formatTransfer";
import type { TransferSnapshot } from "../lib/tauri";
import { ChevronDownIcon, TrashIcon } from "./icons";

const STATE_LABEL: Record<TransferSnapshot["state"], string> = {
  scanning: "Scanning…",
  running: "Transferring",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

const STATE_COLOR: Record<TransferSnapshot["state"], string> = {
  scanning: "text-gray-400",
  running: "text-cyan-300",
  done: "text-green-400",
  failed: "text-red-400",
  cancelled: "text-amber-400",
};

function Bar({ value, state }: { value: number; state: TransferSnapshot["state"] }) {
  const color =
    state === "failed"
      ? "bg-red-500"
      : state === "cancelled"
        ? "bg-amber-500"
        : state === "done"
          ? "bg-green-500"
          : "bg-cyan-500";
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--border)]">
      <div
        className={`h-full rounded-full ${color} transition-[width] duration-200`}
        style={{ width: `${value}%` }}
      />
    </div>
  );
}

function Job({ job }: { job: TransferSnapshot }) {
  const cancel = useTransferStore((s) => s.cancel);
  const active = job.state === "running" || job.state === "scanning";
  const pct = percent(job.bytes_done, job.bytes_total);

  return (
    <div className="border-b border-[var(--border)] px-3 py-2.5 last:border-b-0">
      <div className="mb-1 flex items-baseline gap-2">
        <span className="truncate text-xs font-medium text-gray-200" title={job.label}>
          {job.direction === "download" ? "↓" : "↑"} {job.label}
        </span>
        <span className={`ml-auto shrink-0 text-[11px] ${STATE_COLOR[job.state]}`}>
          {STATE_LABEL[job.state]}
        </span>
        {active ? (
          <button
            onClick={() => void cancel(job.id)}
            className="shrink-0 rounded border border-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-white"
          >
            Cancel
          </button>
        ) : null}
      </div>

      <Bar value={job.state === "done" ? 100 : pct} state={job.state} />

      {/* The four numbers asked for: transferred, elapsed, remaining, and speed. */}
      <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] tabular-nums text-gray-500">
        <span className="text-gray-400">
          {formatBytes(job.bytes_done)}
          {job.bytes_total > 0 ? ` / ${formatBytes(job.bytes_total)}` : ""}
        </span>
        <span>
          {job.files_done}/{job.files_total} file{job.files_total === 1 ? "" : "s"}
        </span>
        <span title="Time elapsed">⏱ {formatDuration(job.elapsed_ms)}</span>
        {active ? (
          <span title="Estimated time remaining">
            ⏳ {job.eta_ms == null ? "—" : formatDuration(job.eta_ms)} left
          </span>
        ) : null}
        {active ? <span title="Throughput">{formatRate(job.bytes_per_sec)}</span> : null}
      </div>

      {job.error ? (
        <div className="mt-1.5 rounded border border-red-500/40 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
          {job.error}
        </div>
      ) : null}

      {/* Per-file rows: what's moving right now, plus anything that failed. */}
      {job.files.length > 0 ? (
        <div className="mt-1.5 space-y-1">
          {job.files.map((f) => (
            <div key={f.remote_path} className="text-[11px]">
              <div className="flex items-baseline gap-2">
                <span
                  className={`truncate ${f.state === "failed" ? "text-red-300" : "text-gray-400"}`}
                  title={f.remote_path}
                >
                  {f.name}
                </span>
                <span className="ml-auto shrink-0 tabular-nums text-gray-600">
                  {formatBytes(f.transferred)}
                  {f.size > 0 ? ` / ${formatBytes(f.size)}` : ""}
                </span>
              </div>
              {f.state === "active" && f.size > 0 ? (
                <div className="mt-0.5">
                  <Bar value={percent(f.transferred, f.size)} state="running" />
                </div>
              ) : null}
              {f.error ? <div className="text-red-400/80">{f.error}</div> : null}
            </div>
          ))}
        </div>
      ) : null}

      {job.state === "done" && job.destination ? (
        <div className="mt-1 truncate text-[11px] text-gray-600" title={job.destination}>
          Saved to {job.destination}
        </div>
      ) : null}
    </div>
  );
}

/**
 * The transfer queue. A floating panel rather than a canvas node, because transfers
 * outlive the SFTP panel that started them and must stay visible across workspace
 * switches.
 */
export function TransfersPanel() {
  const jobs = useTransferStore((s) => s.jobs);
  const open = useTransferStore((s) => s.open);
  const setOpen = useTransferStore((s) => s.setOpen);
  const concurrency = useTransferStore((s) => s.concurrency);
  const setConcurrency = useTransferStore((s) => s.setConcurrency);
  const lastDestination = useTransferStore((s) => s.lastDestination);
  const setLastDestination = useTransferStore((s) => s.setLastDestination);
  const clearFinished = useTransferStore((s) => s.clearFinished);
  const cancel = useTransferStore((s) => s.cancel);

  useEffect(() => {
    let un: (() => void) | undefined;
    void useTransferStore
      .getState()
      .subscribe()
      .then((u) => (un = u));
    return () => un?.();
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  const list = Object.values(jobs).sort((a, b) => {
    const rank = (j: TransferSnapshot) =>
      j.state === "running" || j.state === "scanning" ? 0 : 1;
    return rank(a) - rank(b);
  });
  if (list.length === 0 && !open) return null;

  const activeCount = list.filter(
    (j) => j.state === "running" || j.state === "scanning",
  ).length;

  return createPortal(
    <div className="pointer-events-none fixed bottom-3 right-3 z-40 flex w-[380px] max-w-[calc(100vw-1.5rem)] flex-col items-end">
      {open ? (
        <div className="pointer-events-auto mb-2 max-h-[60vh] w-full overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--surface)] shadow-2xl">
          <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-2">
            <span className="text-xs font-medium text-gray-200">Transfers</span>
            {activeCount > 0 ? (
              <span className="rounded-full bg-cyan-500/20 px-1.5 text-[10px] text-cyan-300">
                {activeCount} active
              </span>
            ) : null}
            {activeCount > 0 ? (
              <button
                type="button"
                onClick={() => {
                  for (const j of list) {
                    if (j.state === "running" || j.state === "scanning") {
                      void cancel(j.id);
                    }
                  }
                }}
                className="rounded px-1.5 py-0.5 text-[10px] text-red-300/90 hover:bg-red-950/40"
                data-tooltip="Cancel all active transfers"
              >
                Cancel all
              </button>
            ) : null}
            <button
              onClick={() => void clearFinished()}
              className="ml-auto rounded p-1 text-gray-500 hover:bg-[var(--border)] hover:text-white"
              data-tooltip="Clear finished"
            >
              <TrashIcon size={13} />
            </button>
            <button
              onClick={() => setOpen(false)}
              className="rounded p-1 text-gray-500 hover:bg-[var(--border)] hover:text-white"
              data-tooltip="Hide"
            >
              <ChevronDownIcon size={14} />
            </button>
          </div>

          <div className="max-h-[38vh] overflow-y-auto">
            {list.length === 0 ? (
              <p className="px-3 py-4 text-center text-[11px] text-gray-500">
                No transfers yet. Right-click a file or folder in an SFTP panel.
              </p>
            ) : (
              list.map((j) => <Job key={j.id} job={j} />)
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2 border-t border-[var(--border)] px-3 py-2 text-[11px] text-gray-500">
            <label className="flex items-center gap-1.5">
              <span>Files at once</span>
              <select
                value={concurrency}
                onChange={(e) => setConcurrency(Number(e.target.value))}
                className="rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[11px] text-gray-200 outline-none"
              >
                {CONCURRENCY_CHOICES.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </label>
            {lastDestination ? (
              <button
                onClick={() => setLastDestination(null)}
                className="ml-auto max-w-[180px] truncate text-left text-gray-600 hover:text-gray-300"
                data-tooltip={`Downloads go to ${lastDestination} — click to be asked again`}
              >
                → {lastDestination}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {!open ? (
        <button
          onClick={() => setOpen(true)}
          className="pointer-events-auto rounded-full border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-xs text-gray-300 shadow-lg hover:text-white"
        >
          {activeCount > 0 ? `${activeCount} transfer${activeCount === 1 ? "" : "s"}…` : "Transfers"}
        </button>
      ) : null}
    </div>,
    document.body,
  );
}
