import { useEffect, useState } from "react";
import type { ReactNode, SelectHTMLAttributes, InputHTMLAttributes, TextareaHTMLAttributes, ButtonHTMLAttributes } from "react";

/** Shared form primitives for every settings section. Defined once, reused everywhere. */

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
    <div className="mb-4 flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <h2 className="text-base font-semibold text-gray-100">{title}</h2>
        {description && (
          <p className="mt-0.5 text-xs leading-relaxed text-gray-500">
            {description}
          </p>
        )}
      </div>
      {action}
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
    <label className="mb-3 block">
      <span className="mb-1 block text-xs font-medium text-gray-300">{label}</span>
      {children}
      {hint && <span className="mt-1 block text-[11px] text-gray-500">{hint}</span>}
    </label>
  );
}

const inputClass =
  "w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-sm text-gray-200 outline-none focus:border-blue-500 disabled:opacity-50";

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
  return <select {...props} className={`${inputClass} ${props.className ?? ""}`} />;
}

export function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      className="flex items-center gap-2 text-xs text-gray-300"
    >
      <span
        className={`relative inline-flex h-5 w-9 items-center rounded-full transition ${
          checked ? "bg-blue-600" : "bg-[var(--border)]"
        }`}
      >
        <span
          className={`inline-block h-4 w-4 transform rounded-full bg-white transition ${
            checked ? "translate-x-4" : "translate-x-0.5"
          }`}
        />
      </span>
      {label}
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
    primary: "bg-blue-600 text-white hover:bg-blue-500",
    ghost:
      "border border-[var(--border)] text-gray-300 hover:bg-[var(--border)] hover:text-gray-100",
    danger:
      "border border-[var(--border)] text-gray-400 hover:bg-red-500/10 hover:text-red-300 hover:border-red-500/40",
  };
  return (
    <button
      {...props}
      className={`inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${variants[variant]} ${className}`}
    />
  );
}

/** Editable markdown document with dirty-tracking and a save button. Reused by
 * the Soul, Memory, and User profile editors. */
export function DocEditor({
  value,
  onSave,
  rows = 16,
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
    <div>
      <TextArea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        rows={rows}
        placeholder={placeholder}
      />
      <div className="mt-2 flex items-center gap-2">
        <Button variant="primary" onClick={save} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save"}
        </Button>
        {dirty ? (
          <span className="text-[11px] text-amber-300">Unsaved changes</span>
        ) : savedAt ? (
          <span className="text-[11px] text-gray-500">Saved</span>
        ) : null}
        <div className="ml-auto">{footer}</div>
      </div>
    </div>
  );
}

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={`rounded-lg border border-[var(--border)] bg-[var(--bg)] p-3 ${className}`}
    >
      {children}
    </div>
  );
}
