import type { AgentConversationMeta } from "../../lib/tauri";

function formatWhen(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso.includes("T") ? iso : `${iso.replace(" ", "T")}Z`);
  if (Number.isNaN(d.getTime())) return iso.slice(0, 10);
  const now = new Date();
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  if (sameDay) {
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}

export function AgentHistory({
  open,
  conversations,
  activeId,
  onSelect,
  onNew,
  onDelete,
  onClose,
}: {
  open: boolean;
  conversations: AgentConversationMeta[];
  activeId: string;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}) {
  if (!open) return null;

  return (
    <div className="border-b border-[var(--border)] bg-[var(--bg)] px-2 py-2">
      <div className="mb-2 flex items-center gap-2 px-1">
        <span className="text-[11px] font-medium text-gray-400">History</span>
        <button
          type="button"
          onClick={onNew}
          className="ml-auto rounded border border-[var(--border)] px-2 py-0.5 text-[10px] text-gray-300 hover:bg-[var(--border)]"
        >
          + New
        </button>
        <button
          type="button"
          onClick={onClose}
          className="text-[10px] text-gray-500 hover:text-gray-300"
        >
          hide
        </button>
      </div>
      <div className="max-h-40 space-y-0.5 overflow-y-auto">
        {conversations.length === 0 && (
          <p className="px-1 py-2 text-[10px] text-gray-600">No saved chats yet.</p>
        )}
        {conversations.map((c) => {
          const active = c.id === activeId;
          return (
            <div
              key={c.id}
              className={`group flex items-start gap-1 rounded px-1.5 py-1 ${
                active ? "bg-blue-600/20" : "hover:bg-[var(--border)]/60"
              }`}
            >
              <button
                type="button"
                className="min-w-0 flex-1 text-left"
                onClick={() => onSelect(c.id)}
              >
                <div className="truncate text-[11px] text-gray-200">{c.title}</div>
                {c.summary && (
                  <div className="mt-0.5 line-clamp-2 text-[10px] leading-snug text-gray-500">
                    {c.summary.replace(/^-\s*/gm, "").slice(0, 120)}
                  </div>
                )}
                <div className="mt-0.5 text-[9px] text-gray-600">{formatWhen(c.updated_at)}</div>
              </button>
              <button
                type="button"
                data-tooltip="Delete"
                onClick={() => onDelete(c.id)}
                className="shrink-0 px-1 text-[10px] text-gray-600 opacity-0 hover:text-red-400 group-hover:opacity-100"
              >
                ✕
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
