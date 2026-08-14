import { useEffect, useMemo, useState } from "react";
import { api, type AgentAnalytics, type ResourceSnapshot } from "../../lib/tauri";
import { DrawerHeader } from "../DrawerHeader";

function Spark({ values, height = 36 }: { values: number[]; height?: number }) {
  if (values.length < 2) {
    return <div className="h-9 text-[10px] text-[var(--text-faint)]">No samples yet</div>;
  }
  const w = 220;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = Math.max(1, max - min);
  const pts = values
    .map((v, i) => {
      const x = (i / (values.length - 1)) * w;
      const y = height - ((v - min) / span) * (height - 4) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg width="100%" height={height} viewBox={`0 0 ${w} ${height}`} className="block" aria-hidden>
      <polyline
        fill="none"
        stroke="var(--accent)"
        strokeWidth="1.4"
        strokeLinejoin="round"
        strokeLinecap="round"
        points={pts}
      />
    </svg>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg)] px-2.5 py-2">
      <div className="text-[10px] uppercase tracking-wide text-[var(--text-faint)]">{label}</div>
      <div className="mt-0.5 font-mono text-[13px] text-[var(--text)]">{value}</div>
      {hint ? <div className="mt-0.5 text-[10px] text-[var(--text-faint)]">{hint}</div> : null}
    </div>
  );
}

export function AnalyticsPanel({ width }: { width: number }) {
  const [data, setData] = useState<AgentAnalytics | null>(null);
  const [samples, setSamples] = useState<ResourceSnapshot[]>([]);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () => {
      api
        .agentAnalytics()
        .then((a) => {
          if (!alive) return;
          setData(a);
          setErr(null);
          setSamples((prev) => [...prev.slice(-119), a.resource]);
        })
        .catch((e: unknown) => {
          if (alive) setErr(String(e));
        });
    };
    load();
    const t = window.setInterval(load, 4000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);

  const cachePcts = useMemo(() => (data?.cache ?? []).map((p) => p.pct), [data]);
  const cpu = useMemo(() => samples.map((s) => s.cpu_pct), [samples]);
  const ram = useMemo(() => samples.map((s) => s.process_ram_mb), [samples]);
  const last = samples[samples.length - 1] ?? data?.resource;

  return (
    <aside className="xc-drawer flex flex-col" style={{ width }} aria-label="Agent analytics">
      <DrawerHeader title="Analytics" />
      <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-4">
        {err ? (
          <p className="mt-3 text-[12px] text-[var(--danger)]">{err}</p>
        ) : null}
        {!data ? (
          <p className="mt-3 text-[12px] text-[var(--text-faint)]">Loading…</p>
        ) : (
          <div className="flex flex-col gap-3 pt-2">
            <div className="grid grid-cols-2 gap-2">
              <Stat
                label="Cache hit"
                value={`${data.cache_avg_pct.toFixed(0)}%`}
                hint={`${data.cache.length} recent turns`}
              />
              <Stat
                label="This process"
                value={`${last?.process_ram_mb ?? 0} MB`}
                hint={last ? `CPU ${last.cpu_pct.toFixed(0)}%` : undefined}
              />
              <Stat
                label="System RAM"
                value={
                  last
                    ? `${Math.round((last.ram_mb / Math.max(1, last.ram_total_mb)) * 100)}%`
                    : "—"
                }
                hint={last ? `${last.ram_mb} / ${last.ram_total_mb} MB` : undefined}
              />
              <Stat
                label="GPU"
                value={
                  last?.gpu_pct != null ? `${last.gpu_pct.toFixed(0)}%` : "n/a"
                }
                hint={
                  last?.gpu_name
                    ? `${last.gpu_name}${last.gpu_mem_mb != null ? ` · ${last.gpu_mem_mb} MB` : ""}`
                    : "no nvidia-smi"
                }
              />
            </div>

            <section>
              <h3 className="mb-1 text-[11px] font-medium text-[var(--text-dim)]">
                Cache hit rate (recent turns)
              </h3>
              <Spark values={cachePcts} />
            </section>
            <section>
              <h3 className="mb-1 text-[11px] font-medium text-[var(--text-dim)]">
                App CPU / RAM while this page is open
              </h3>
              <Spark values={cpu} />
              <Spark values={ram} />
              <p className="mt-1 text-[10px] text-[var(--text-faint)]">
                CSS-only charts — no WebGL. Spikes here usually mean a live agent turn
                or too many canvas terminals.
              </p>
            </section>

            <section>
              <h3 className="mb-1 text-[11px] font-medium text-[var(--text-dim)]">
                Tools across recent chats
              </h3>
              <ul className="flex flex-col gap-0.5">
                {data.tools_all.slice(0, 12).map((t) => (
                  <li
                    key={t.name}
                    className="flex items-center justify-between font-mono text-[11px] text-[var(--text-dim)]"
                  >
                    <span className="truncate">{t.name}</span>
                    <span className="text-[var(--text-faint)]">{t.count}</span>
                  </li>
                ))}
                {data.tools_all.length === 0 ? (
                  <li className="text-[11px] text-[var(--text-faint)]">No tool history yet</li>
                ) : null}
              </ul>
            </section>

            <section>
              <h3 className="mb-1 text-[11px] font-medium text-[var(--text-dim)]">
                Sessions
              </h3>
              <ul className="flex flex-col gap-1.5">
                {data.conversations.map((c) => (
                  <li
                    key={c.id}
                    className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5"
                  >
                    <div className="truncate text-[12px] text-[var(--text)]">{c.title || "Untitled"}</div>
                    <div className="mt-0.5 text-[10px] text-[var(--text-faint)]">
                      {c.user_turns} user · {c.tool_calls} tools
                      {c.updated_at ? ` · ${c.updated_at.slice(0, 16)}` : ""}
                    </div>
                    {c.tools[0] ? (
                      <div className="mt-0.5 truncate font-mono text-[10px] text-[var(--text-faint)]">
                        top {c.tools[0].name} ×{c.tools[0].count}
                      </div>
                    ) : null}
                  </li>
                ))}
              </ul>
            </section>
          </div>
        )}
      </div>
    </aside>
  );
}
