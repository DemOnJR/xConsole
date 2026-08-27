import { useHarnessStore } from "../../../stores/harnessStore";
import { useAgentStore } from "../../../stores/agentStore";
import { useSettingsStore } from "../../../stores/settingsStore";
import { formatSessionCache, sessionCacheFromMessages } from "../../../lib/streamStats";

export function ContextMemoryModule() {
  const showContext = useHarnessStore((s) => s.showContext);
  const messages = useAgentStore((s) => s.messages);
  const provider = useSettingsStore((s) => s.get("ai_provider", "deepseek"));
  const model = useSettingsStore((s) => s.get("ai_model", "deepseek-reasoner"));

  if (!showContext) return null;

  const cacheStats = sessionCacheFromMessages(messages);
  const cacheFormatted = formatSessionCache(cacheStats);

  let totalChars = 0;
  for (const m of messages) {
    totalChars += m.content?.length || 0;
  }
  const approxTokens = Math.round(totalChars / 3.8);

  return (
    <div className="border-b border-[var(--border)] bg-[var(--surface)]/60 text-xs font-mono px-3 py-1.5 flex items-center justify-between gap-4 select-none">
      <div className="flex items-center gap-3 min-w-0 flex-wrap">
        <div className="flex items-center gap-1 text-[var(--text-dim)]">
          <span className="text-violet-400 font-semibold">🧠</span>
          <span className="text-[10px] uppercase tracking-wider font-semibold">CONTEXT:</span>
        </div>

        <div className="flex items-center gap-2">
          <span className="text-[11px] text-[var(--text)] font-semibold">
            {approxTokens.toLocaleString()} tokens
          </span>
          <span className="text-[10px] text-[var(--text-dim)]">
            ({messages.length} msgs)
          </span>
        </div>

        {cacheFormatted && (
          <div className="flex items-center gap-1 rounded bg-violet-500/10 text-violet-300 border border-violet-500/30 px-1.5 py-0.2 text-[10px]">
            <span>⚡ {cacheFormatted}</span>
          </div>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0 text-[10px] text-[var(--text-dim)]">
        <span className="truncate max-w-[140px] font-mono" title={`${provider} / ${model}`}>
          {model || provider}
        </span>
      </div>
    </div>
  );
}
