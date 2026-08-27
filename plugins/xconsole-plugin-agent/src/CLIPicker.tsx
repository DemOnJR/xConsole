import { useEffect, useMemo, useRef, useState } from "react";

export interface CLIPickerOption {
  id: string;
  label: string;
  detail?: string;
  /** When true, the option is "selected" (for multi-select pickers). */
  selected?: boolean;
  /** Optional action button on the row (e.g. "Rename"). */
  actionLabel?: string;
  onAction?: (option: CLIPickerOption, e: React.MouseEvent | React.PointerEvent) => void;
}

/**
 * In-console arrow-key picker. Rendered above the prompt line.
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
  const pickerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (multi) {
      setSelected(new Set(options.filter((o) => o.selected).map((o) => o.id)));
    }
  }, [options, multi]);

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

  // Global escape & outside click handlers to ensure reliable closing
  useEffect(() => {
    const onGlobalKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onCancel();
      }
    };
    const onPointerOutside = (e: MouseEvent | PointerEvent) => {
      if (pickerRef.current && !pickerRef.current.contains(e.target as Node)) {
        onCancel();
      }
    };
    window.addEventListener("keydown", onGlobalKey, true);
    // Use pointerdown so clicking chat window / canvas immediately closes
    window.addEventListener("pointerdown", onPointerOutside);
    return () => {
      window.removeEventListener("keydown", onGlobalKey, true);
      window.removeEventListener("pointerdown", onPointerOutside);
    };
  }, [onCancel]);

  const confirm = () => {
    const opt = filtered[index];
    if (!opt) return;
    if (multi) {
      const next = new Set(selected);
      if (next.has(opt.id)) next.delete(opt.id);
      else next.add(opt.id);
      setSelected(next);
      onPick(opt);
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
      if (multi) {
        onPick({ id: "__done__", label: "Done", selected: selected.size > 0 });
      } else {
        confirm();
      }
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
    <div
      ref={pickerRef}
      data-picker
      className="nodrag nopan nowheel rounded-md border border-[var(--border-strong)] bg-[var(--surface)] shadow-2xl"
      onPointerDown={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
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
        {/* X Close button */}
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onCancel();
          }}
          className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-[11px] text-gray-500 hover:bg-[var(--border)] hover:text-white"
          title="Close (Esc)"
          aria-label="Close"
        >
          ✕
        </button>
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
              role="option"
              aria-selected={active}
              onMouseEnter={() => {
                const i = filtered.findIndex((x) => x.id === o.id);
                if (i >= 0) setIndex(i);
              }}
              onPointerDown={(e) => {
                const target = e.target as HTMLElement;
                if (target.closest("[data-row-action]")) return;
                e.preventDefault();
                e.stopPropagation();
                if (e.button !== 0) return;
                if (multi) {
                  const next = new Set(selected);
                  if (next.has(o.id)) next.delete(o.id);
                  else next.add(o.id);
                  setSelected(next);
                  onPick(o);
                } else {
                  onPick(o);
                }
              }}
              onMouseDown={(e) => {
                e.preventDefault();
                e.stopPropagation();
              }}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
              }}
              className={`flex items-center gap-2 px-2.5 py-1 font-mono text-[11px] ${
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
              {o.actionLabel && o.onAction && (
                <button
                  type="button"
                  data-row-action
                  onPointerDown={(e) => {
                    e.stopPropagation();
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    o.onAction!(o, e);
                  }}
                  className="shrink-0 rounded border border-[var(--border)] px-1.5 py-0.2 text-[9px] text-[var(--text-faint)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
                  title={o.actionLabel}
                >
                  {o.actionLabel}
                </button>
              )}
            </div>
          );
        })}
      </div>

      {multi && (
        <div className="border-t border-[var(--border)] px-2.5 py-1.5">
          <button
            type="button"
            onPointerDown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (e.button !== 0) return;
              onPick({ id: "__done__", label: "Done", selected: selected.size > 0 });
            }}
            onMouseDown={(e) => {
              e.preventDefault();
              e.stopPropagation();
            }}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
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
