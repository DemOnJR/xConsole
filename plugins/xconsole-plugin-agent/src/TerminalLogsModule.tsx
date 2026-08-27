export function TerminalLogsModule() {
  return (
    <div className="border-b border-[var(--border)] bg-black/70 px-3 py-2.5 text-xs font-mono">
      <div className="flex items-center justify-between pb-1 border-b border-white/10">
        <span className="text-[10px] text-emerald-400 font-semibold uppercase">📜 Harness Terminal Stream</span>
        <span className="text-[10px] text-zinc-500">Live stdout/stderr</span>
      </div>
      <pre className="mt-2 text-[11px] text-zinc-400 leading-relaxed overflow-x-auto">
        <code>
          {`[agent-core] Initialized LLM provider channel
[tools] Auto-discovered 8 tools from manifest
[harness] Spatiotemporal fork active. Ready.`}
        </code>
      </pre>
    </div>
  );
}
