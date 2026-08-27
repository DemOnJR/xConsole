export function ContextMemoryModule() {
  return (
    <div className="border-b border-[var(--border)] bg-[var(--surface)] px-3 py-2.5 text-xs font-mono">
      <div className="flex items-center justify-between pb-1.5 border-b border-white/5">
        <span className="text-[10px] text-violet-400 font-semibold uppercase">🧠 Context &amp; Memory State</span>
        <span className="text-[10px] text-zinc-500">26% (32,410 / 128,000 tokens)</span>
      </div>
      <div className="mt-2 grid grid-cols-2 gap-2 text-[11px]">
        <div className="rounded bg-[var(--surface-2)] p-2">
          <div className="text-zinc-500 text-[10px]">Session Turns:</div>
          <div className="text-gray-200 font-semibold mt-0.5">14 interactions</div>
        </div>
        <div className="rounded bg-[var(--surface-2)] p-2">
          <div className="text-zinc-500 text-[10px]">Active Buffer:</div>
          <div className="text-gray-200 font-semibold mt-0.5">Workspace KV Store</div>
        </div>
      </div>
    </div>
  );
}
