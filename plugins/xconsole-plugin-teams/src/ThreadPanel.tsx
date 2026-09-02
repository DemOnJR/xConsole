import { useState } from "react";
import type { AgentMessage, Persona } from "../../../src/lib/tauri";
import { CloseIcon, SendIcon } from "../../../src/components/icons";
import { avatarColor, initials, nameOf } from "./channels";
import { parentSummary, type ThreadParent } from "./threads";

function clock(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date((iso || "").replace(" ", "T"));
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/**
 * The thread drawer: a suggestion or a correction attached to one message or one log
 * line, with its own composer.
 *
 * It has its own composer deliberately. Sharing the room's composer is how a reply ends
 * up posted to the room instead -- the two look identical and the only difference is
 * which one had focus.
 */
export function ThreadPanel({
  parent,
  parentId,
  replies,
  personas,
  onClose,
  onSend,
}: {
  parent: ThreadParent | null;
  parentId: string;
  replies: AgentMessage[];
  personas: Persona[];
  onClose: () => void;
  onSend: (body: string) => Promise<void>;
}) {
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);

  const submit = async () => {
    const body = draft.trim();
    if (!body || sending) return;
    setSending(true);
    try {
      await onSend(body);
      setDraft("");
    } finally {
      setSending(false);
    }
  };

  return (
    <aside className="flex w-[300px] shrink-0 flex-col border-l border-[var(--border)] bg-[var(--surface-2)]">
      <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-2">
        <h3 className="flex-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
          Thread
        </h3>
        <button type="button" className="xc-icon-btn" data-tooltip="Close thread" onClick={onClose}>
          <CloseIcon size={14} />
        </button>
      </div>

      <div className="border-b border-[var(--border)] px-3 py-2">
        {parent ? (
          <>
            <div className="mb-1 flex items-center gap-1.5">
              <span className="text-[11px] font-semibold text-[var(--text)]">
                {parent.kind === "log"
                  ? nameOf(personas, parent.entry.persona_id)
                  : nameOf(personas, parent.message.from_id)}
              </span>
              <span className="text-[10px] uppercase tracking-wide text-[var(--text-faint)]">
                {parent.kind === "log" ? "log line" : "message"}
              </span>
            </div>
            <p
              className={`whitespace-pre-wrap break-words text-[11px] leading-relaxed text-[var(--text-dim)] ${
                parent.kind === "log" ? "font-mono" : ""
              }`}
            >
              {parentSummary(parent)}
            </p>
          </>
        ) : (
          // The anchor can be a live log line that was never persisted, or a row pruned
          // since. Say so rather than showing a blank header: the replies below are real
          // either way, and they stay in the channel.
          <p className="text-[11px] leading-relaxed text-[var(--text-faint)]">
            The line this thread hangs off is no longer loaded. Replies below are still in
            the channel.
          </p>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
        {replies.length === 0 ? (
          <p className="py-6 text-center text-[11px] text-[var(--text-faint)]">
            No replies yet. Add a suggestion or a correction.
          </p>
        ) : (
          replies.map((m) => {
            const from = nameOf(personas, m.from_id);
            return (
              <div key={m.id} className="mb-2.5 flex gap-2">
                <span
                  className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[9px] font-semibold text-[var(--bg)]"
                  style={{ background: avatarColor(m.from_id || "you") }}
                >
                  {initials(from)}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    <span className="text-[11px] font-semibold text-[var(--text)]">{from}</span>
                    <span className="font-mono text-[10px] text-[var(--text-faint)]">
                      {clock(m.created_at)}
                    </span>
                  </div>
                  <p className="whitespace-pre-wrap break-words text-[12px] leading-relaxed text-[var(--text)]">
                    {m.body}
                  </p>
                </div>
              </div>
            );
          })
        )}
      </div>

      <form
        className="flex items-end gap-2 border-t border-[var(--border)] px-2 py-2"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submit();
            }
          }}
          rows={1}
          placeholder="Reply in thread"
          aria-label={`Reply in thread ${parentId}`}
          className="max-h-28 min-h-[32px] min-w-0 flex-1 resize-none rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)]"
        />
        <button
          type="submit"
          disabled={sending || !draft.trim()}
          className="xc-icon-btn mb-0.5"
          data-tooltip="Send"
        >
          <SendIcon size={13} />
        </button>
      </form>
    </aside>
  );
}
