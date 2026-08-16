import type { QueuedMessage } from "../../stores/agentStore";
import { previewSrc } from "../../lib/vision";

export function QueuedMessages({
  items,
  onChange,
  onRemove,
  onSendNow,
  canSendNow,
}: {
  items: QueuedMessage[];
  onChange: (id: string, text: string) => void;
  onRemove: (id: string) => void;
  onSendNow?: (id: string) => void;
  canSendNow?: boolean;
}) {
  if (items.length === 0) return null;

  return (
    <div className="border-t border-[var(--border)]/80 bg-[var(--surface)]/40 px-3 py-2">
      <div className="mb-1.5 flex items-center justify-between text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
        <span>
          {items.length} queued — edit before {items.length === 1 ? "it sends" : "they send"}
        </span>
      </div>
      <div className="flex flex-col gap-1.5">
        {items.map((item, index) => (
          <div
            key={item.id}
            className="flex items-start gap-2 rounded-md border border-dashed border-[var(--border-strong)] bg-[var(--bg)] px-2 py-1.5"
          >
            <span className="shrink-0 select-none font-mono text-[12px] leading-[20px] text-[var(--text-faint)]">
              ~#
            </span>
            <div className="min-w-0 flex-1">
              <textarea
                value={item.text}
                rows={Math.min(4, Math.max(1, item.text.split("\n").length))}
                onChange={(e) => onChange(item.id, e.target.value)}
                aria-label={`Queued message ${index + 1}`}
                className="max-h-24 w-full resize-none border-0 bg-transparent p-0 font-mono text-[12px] leading-[20px] text-[var(--text)] outline-none"
              />
              {item.images && item.images.length > 0 ? (
                <div className="mt-1 flex flex-wrap gap-1">
                  {item.images.map((img, i) => (
                    <img
                      key={`${item.id}-${i}`}
                      src={previewSrc(img)}
                      alt={img.name || `Image #${i + 1}`}
                      className="h-10 max-w-[72px] rounded border border-[var(--border)] object-cover"
                    />
                  ))}
                </div>
              ) : null}
            </div>
            <div className="flex shrink-0 flex-col gap-1">
              {canSendNow && onSendNow && index === 0 ? (
                <button
                  type="button"
                  onClick={() => onSendNow(item.id)}
                  className="rounded px-1.5 py-0.5 text-[10px] text-cyan-300 hover:bg-[var(--border)]"
                  data-tooltip="Send this now"
                >
                  send
                </button>
              ) : (
                <span className="px-1.5 py-0.5 text-[10px] text-[var(--text-faint)]">
                  #{index + 1}
                </span>
              )}
              <button
                type="button"
                onClick={() => onRemove(item.id)}
                className="rounded px-1.5 py-0.5 text-[10px] text-[var(--text-faint)] hover:bg-[var(--border)] hover:text-red-300"
                data-tooltip="Remove from queue"
                aria-label="Remove queued message"
              >
                ✕
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
