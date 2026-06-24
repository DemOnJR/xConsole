import { useEffect, useRef, useState } from "react";
import { useDialogStore } from "../stores/dialogStore";

/** Renders the active in-app dialog (styled confirm / prompt). Mount once. */
export function DialogHost() {
  const active = useDialogStore((s) => s.active);
  const settle = useDialogStore((s) => s.settle);
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (active?.kind === "prompt") {
      setValue(active.defaultValue ?? "");
      // Focus + select after the modal mounts.
      setTimeout(() => inputRef.current?.select(), 30);
    }
  }, [active]);

  if (!active) return null;

  const isPrompt = active.kind === "prompt";
  const confirmValue: boolean | string = isPrompt ? value : true;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) settle(isPrompt ? null : false);
      }}
    >
      <div
        className="w-[min(420px,92vw)] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 shadow-2xl"
        onKeyDown={(e) => {
          if (e.key === "Escape") settle(isPrompt ? null : false);
          if (e.key === "Enter" && (isPrompt || true)) {
            e.preventDefault();
            settle(confirmValue);
          }
        }}
      >
        <div className="mb-1 text-sm font-medium text-[var(--text)]">{active.title}</div>
        {active.message && (
          <p className="mb-3 whitespace-pre-wrap text-xs leading-relaxed text-[var(--text-dim)]">
            {active.message}
          </p>
        )}
        {isPrompt && (
          <div className="mb-3">
            {active.label && (
              <label className="mb-1 block text-[11px] text-[var(--text-dim)]">
                {active.label}
              </label>
            )}
            <input
              ref={inputRef}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={active.placeholder}
              className="w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-sm text-[var(--text)] outline-none focus:border-[var(--accent)]"
            />
          </div>
        )}
        <div className="mt-3 flex justify-end gap-2">
          <button
            onClick={() => settle(isPrompt ? null : false)}
            className="rounded-md border border-[var(--border)] px-3 py-1.5 text-xs text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
          >
            {active.cancelText ?? "Cancel"}
          </button>
          <button
            onClick={() => settle(confirmValue)}
            disabled={isPrompt && !value.trim()}
            className={`rounded-md px-3 py-1.5 text-xs font-medium text-white disabled:opacity-40 ${
              active.danger
                ? "bg-red-600 hover:bg-red-500"
                : "bg-[var(--accent)] text-[var(--accent-fg)] hover:opacity-90"
            }`}
          >
            {active.confirmText ?? (isPrompt ? "OK" : "Confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
