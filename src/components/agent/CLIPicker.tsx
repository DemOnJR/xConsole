import { useEffect, useMemo, useRef, useState } from "react";

export interface CLIPickerOption {
  id: string;
  label: string;
  detail?: string;
  /** When true, the option is "selected" (for multi-select pickers). */
  selected?: boolean;
}

/**
 * In-console arrow-key picker (opencode-style). Rendered above the prompt line.
 * Keyboard: type to filter, ↑/↓ to move, Enter to pick, Esc to cancel.
 * For multi-select (targets): Space toggles, Enter confirms.
 */
export function CLIPicker({
  title,
  options,
  onPick,
  onCancel,
  multi = false,
  placeholder,
}: {
  title: string;
  options: CLIPickerOption[];
  onPick: (option: CLIPickerOption) => void;
  onCancel: () => void;
  multi?: boolean;
  placeholder?: string;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(options.filter((o) => o.selected).map((o) => o.id)),
  );
  const inputRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter(
      (o) => o.label.toLowerCase().includes(q) || (o.detail ?? "").toLowerCase().includes(q),
    );
  }, [options, query]);

  useEffect(() => {
    setIndex(0);
  }, [query]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const confirm = () => {
    const opt = filtered[index];
    if (!opt) return;
    if (multi) {
      const next = new Set(selected);
      if (next.has(opt.id)) next.delete(opt.id);
      else next.add(opt.id);
      setSelected(next);
      return;
    }
    onPick(opt);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex((i) => Math.min(filtered.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (!multi) confirm();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    } else if (e.key === " " && multi) {
      e.preventDefault();
      confirm();
    }
  };

  const visible = filtered.slice(0, 12);

  return (
    <div className="rounded-md border border-[var(--border-strong)] bg-[var(--surface)] shadow-2xl">
      <div className="flex items-center gap-2 border-b border-[var(--border)] px-2.5 py-1.5">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
          {title}
        </span>
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={placeholder ?? "Filter…"}
          className="min-w-0 flex-1 border-0 bg-transparent font-mono text-[11px] text-[var(--text)] outline-none placeholder:text-[var(--text-faint)]"
        />
        <span className="text-[10px] text-[var(--text-faint)]">
          {multi ? "space=select · enter=done" : "↑↓ · enter"}
        </span>
      </div>
      <div className="max-h-48 overflow-y-auto py-0.5">
        {visible.length === 0 && (
          <div className="px-2.5 py-2 text-[11px] text-[var(--text-faint)]">No matches</div>
        )}
        {visible.map((o) => {
          const active = o.id === filtered[index]?.id;
          const isSel = selected.has(o.id);
          return (
            <div
              key={o.id}
              onMouseEnter={() => {
                const i = filtered.findIndex((x) => x.id === o.id);
                if (i >= 0) setIndex(i);
              }}
              onMouseDown={(e) => {
                e.preventDefault();
                if (multi) {
                  const next = new Set(selected);
                  if (next.has(o.id)) next.delete(o.id);
                  else next.add(o.id);
                  setSelected(next);
                } else {
                  onPick(o);
                }
              }}
              className={`flex cursor-pointer items-center gap-2 px-2.5 py-1 font-mono text-[11px] ${
                active ? "bg-[var(--border)] text-[var(--text)]" : "text-[var(--text-dim)]"
              }`}
            >
              {multi && (
                <span className={isSel ? "text-emerald-400" : "text-[var(--text-faint)]"}>
                  {isSel ? "●" : "○"}
                </span>
              )}
              <span className="truncate">{o.label}</span>
              {o.detail && (
                <span className="ml-auto truncate text-[10px] text-[var(--text-faint)]">
                  {o.detail}
                </span>
              )}
            </div>
          );
        })}
      </div>
      {multi && (
        <div className="border-t border-[var(--border)] px-2.5 py-1.5">
          <button
            type="button"
            onMouseDown={(e) => {
              e.preventDefault();
              onPick({ id: "__done__", label: "Done", selected: selected.size > 0 });
            }}
            className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-center text-[10px] text-[var(--text-dim)] hover:text-[var(--text)]"
          >
            Done ({selected.size} selected)
          </button>
        </div>
      )}
    </div>
  );
}
