import { useEffect, useState } from "react";
import type {
  ReactNode,
  SelectHTMLAttributes,
  InputHTMLAttributes,
  TextareaHTMLAttributes,
  ButtonHTMLAttributes,
} from "react";

/** Shared form primitives for settings. Clean, enterprise-grade, zero visual clutter. */

export function SectionHeader({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="mb-6 flex items-start justify-between gap-4 pb-3 border-b border-[var(--border)]">
      <div className="min-w-0 flex-1">
        <h2 className="text-sm font-semibold tracking-wide text-gray-100 uppercase">
          {title}
        </h2>
        {description && (
          <p className="mt-1 text-xs leading-relaxed text-gray-400">
            {description}
          </p>
        )}
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </div>
  );
}

export function SettingsGroup({
  title,
  description,
  children,
  className = "",
}: {
  title?: string;
  description?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`space-y-4 ${className}`}>
      {title && (
        <div className="mb-2">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-gray-300">
            {title}
          </h3>
          {description && (
            <p className="mt-0.5 text-[11px] text-gray-400">{description}</p>
          )}
        </div>
      )}
      <div className="space-y-3">{children}</div>
    </div>
  );
}

export function SettingsRow({
  label,
  description,
  children,
  className = "",
}: {
  label: string;
  description?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`flex items-center justify-between gap-4 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3.5 transition hover:border-[var(--border-strong)] ${className}`}
    >
      <div className="min-w-0 flex-1">
        <div className="text-xs font-medium text-gray-200">{label}</div>
        {description && (
          <div className="mt-0.5 text-[11px] leading-normal text-gray-400">
            {description}
          </div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="block space-y-1.5">
      <span className="block text-xs font-medium text-gray-300">{label}</span>
      {children}
      {hint && <span className="block text-[11px] leading-relaxed text-gray-400">{hint}</span>}
    </label>
  );
}

const inputClass =
  "w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-3 py-1.5 text-xs text-gray-200 placeholder-gray-500 outline-none transition focus:border-cyan-500/60 focus:ring-1 focus:ring-cyan-500/30 disabled:opacity-50";

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={`${inputClass} ${props.className ?? ""}`} />;
}

export function TextArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={`${inputClass} resize-y font-mono text-xs leading-relaxed ${props.className ?? ""}`}
    />
  );
}

export function Select(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={`${inputClass} cursor-pointer py-1.5 ${props.className ?? ""}`}
    />
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => {
        if (!disabled) onChange(!checked);
      }}
      className={`inline-flex items-center gap-2 text-xs text-gray-300 transition select-none ${
        disabled ? "cursor-not-allowed opacity-40" : ""
      }`}
    >
      <span
        className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors ${
          checked ? "bg-cyan-500" : "bg-zinc-700"
        }`}
      >
        <span
          className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow transition-transform ${
            checked ? "translate-x-4" : "translate-x-1"
          }`}
        />
      </span>
      {label && <span>{label}</span>}
    </button>
  );
}

type ButtonVariant = "primary" | "ghost" | "danger";

export function Button({
  variant = "ghost",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  const variants: Record<ButtonVariant, string> = {
    primary:
      "bg-cyan-600 text-white hover:bg-cyan-500 border border-cyan-500/40 shadow-xs",
    ghost:
      "border border-[var(--border)] bg-[var(--surface)] text-gray-300 hover:bg-[var(--border)] hover:text-white",
    danger:
      "border border-red-500/30 bg-red-500/10 text-red-400 hover:bg-red-500/20 hover:text-red-300",
  };
  return (
    <button
      {...props}
      className={`inline-flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${variants[variant]} ${className}`}
    />
  );
}

/** Card container with minimal 1px border and refined background */
export function Card({
  children,
  className = "",
  onClick,
}: {
  children: ReactNode;
  className?: string;
  onClick?: (e: React.MouseEvent<HTMLDivElement>) => void;
}) {
  return (
    <div
      onClick={onClick}
      className={`rounded-lg border border-[var(--border)] bg-[var(--surface)] p-4 transition ${className}`}
    >
      {children}
    </div>
  );
}

/** Editable document with dirty-tracking and save button */
export function DocEditor({
  value,
  onSave,
  rows = 14,
  placeholder,
  footer,
}: {
  value: string;
  onSave: (next: string) => Promise<void> | void;
  rows?: number;
  placeholder?: string;
  footer?: ReactNode;
}) {
  const [draft, setDraft] = useState(value);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  useEffect(() => {
    setDraft(value);
  }, [value]);

  const dirty = draft !== value;

  const save = async () => {
    setSaving(true);
    try {
      await onSave(draft);
      setSavedAt(Date.now());
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-3">
      <TextArea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        rows={rows}
        placeholder={placeholder}
      />
      <div className="flex items-center gap-2">
        <Button variant="primary" onClick={save} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save"}
        </Button>
        {dirty ? (
          <span className="text-[11px] text-amber-400 font-mono">Unsaved changes</span>
        ) : savedAt ? (
          <span className="text-[11px] text-emerald-400 font-mono">Saved ✓</span>
        ) : null}
        <div className="ml-auto">{footer}</div>
      </div>
    </div>
  );
}
