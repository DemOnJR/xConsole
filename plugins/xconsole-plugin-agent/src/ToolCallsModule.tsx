export function ToolCallsModule() {
  const sampleTools = [
    { name: "execute_bash", status: "success", call: "cargo test", duration: "1.2s" },
    { name: "edit_file", status: "success", call: "src/lib.rs (lines 45-60)", duration: "180ms" },
    { name: "search_code", status: "success", call: "query: 'harness'", duration: "45ms" },
  ];

  return (
    <div className="border-b border-[var(--border)] bg-[var(--surface)] px-3 py-2.5 text-xs font-mono">
      <div className="flex items-center justify-between pb-1.5 border-b border-white/5">
        <span className="text-[10px] text-cyan-400 font-semibold uppercase">⚡ Tool Invocations &amp; Telemetry</span>
        <span className="text-[10px] text-zinc-500">Auto-dispatched via Cordis kernel</span>
      </div>
      <div className="mt-2 space-y-1.5">
        {sampleTools.map((t, i) => (
          <div key={i} className="flex items-center justify-between rounded bg-[var(--surface-2)] px-2 py-1 text-[11px]">
            <div className="flex items-center gap-2">
              <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
              <span className="font-bold text-gray-200">{t.name}</span>
              <span className="text-zinc-500 truncate max-w-[200px]">{t.call}</span>
            </div>
            <span className="text-[10px] text-zinc-500">{t.duration}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
