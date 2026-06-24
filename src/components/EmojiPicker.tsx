import { useMemo, useState } from "react";
import { EMOJI_CATEGORIES, ALL_EMOJIS } from "../lib/emojis";

export function EmojiPicker({
  value,
  onPick,
}: {
  value?: string;
  onPick: (emoji: string) => void;
}) {
  const [q, setQ] = useState("");

  const results = useMemo(() => {
    const term = q.trim().toLowerCase();
    if (!term) return null;
    const seen = new Set<string>();
    return ALL_EMOJIS.filter((e) => {
      if (seen.has(e.c)) return false;
      const hit = e.k.includes(term) || e.c === term;
      if (hit) seen.add(e.c);
      return hit;
    });
  }, [q]);

  const Cell = ({ char }: { char: string }) => (
    <button
      type="button"
      onClick={() => onPick(char)}
      className={`flex h-7 w-7 items-center justify-center rounded text-lg hover:bg-[var(--border)] ${
        value === char ? "bg-[var(--border)] ring-1 ring-blue-500" : ""
      }`}
    >
      {char}
    </button>
  );

  return (
    <div className="flex flex-col">
      <input
        autoFocus
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Search icons (e.g. server, cloud, rust)…"
        className="mb-2 w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-xs text-gray-200 outline-none focus:border-blue-500"
      />

      <div className="max-h-44 overflow-y-auto pr-1">
        {results ? (
          results.length > 0 ? (
            <div className="grid grid-cols-7 gap-0.5">
              {results.map((e) => (
                <Cell key={e.c} char={e.c} />
              ))}
            </div>
          ) : (
            <p className="px-1 py-3 text-center text-[11px] text-gray-600">
              No icons match “{q}”.
            </p>
          )
        ) : (
          EMOJI_CATEGORIES.map((cat) => (
            <div key={cat.name} className="mb-1.5">
              <div className="sticky top-0 bg-[var(--surface)] px-0.5 py-0.5 text-[10px] uppercase tracking-wider text-gray-500">
                {cat.name}
              </div>
              <div className="grid grid-cols-7 gap-0.5">
                {cat.items.map((e) => (
                  <Cell key={e.c} char={e.c} />
                ))}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
