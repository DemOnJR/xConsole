import { useEffect, useMemo, useState } from "react";
import { api, type AgentAnalytics, type ResourceSnapshot } from "../../lib/tauri";
import { useUiStore } from "../../stores/uiStore";

function Spark({
  values,
  height = 96,
  color = "var(--accent)",
}: {
  values: number[];
  height?: number;
  color?: string;
}) {
  if (values.length < 2) {
    return (
      <div className="flex h-24 items-center text-[12px] text-[var(--text-faint)]">
        No samples yet — leave this page open to collect them.
      </div>
    );
  }
  const w = 640;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = Math.max(1e-6, max - min);
  const pts = values
    .map((v, i) => {
      const x = (i / (values.length - 1)) * w;
      const y = height - ((v - min) / span) * (height - 8) - 4;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg
      width="100%"
      height={height}
      viewBox={`0 0 ${w} ${height}`}
      preserveAspectRatio="none"
      className="block"
      aria-hidden
    >
      <polyline
        fill="none"
        stroke={color}
        strokeWidth="2"
        strokeLinejoin="round"
        strokeLinecap="round"
        points={pts}
      />
    </svg>
  );
}

function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface)] px-4 py-3">
      <div className="text-[11px] uppercase tracking-wide text-[var(--text-faint)]">{label}</div>
      <div className="mt-1 font-mono text-[22px] leading-tight text-[var(--text)]">{value}</div>
      {hint ? <div className="mt-1 text-[12px] text-[var(--text-faint)]">{hint}</div> : null}
    </div>
  );
}

function gpuHint(last: ResourceSnapshot | undefined): string {
  if (!last) return "detecting…";
  const bits: string[] = [];
  if (last.gpu_name) bits.push(last.gpu_name);
  if (last.gpu_mem_mb != null && last.gpu_mem_total_mb != null) {
    bits.push(`${last.gpu_mem_mb} / ${last.gpu_mem_total_mb} MB`);
  } else if (last.gpu_mem_total_mb != null) {
    bits.push(`${last.gpu_mem_total_mb} MB dedicated`);
  } else if (last.gpu_mem_mb != null) {
    bits.push(`${last.gpu_mem_mb} MB in use`);
  } else if (last.gpu_name && /intel/i.test(last.gpu_name) && last.gpu_mem_total_mb == null) {
    bits.push("shared memory");
  }
  if (bits.length === 0) return "no GPU reported";
  return bits.join(" · ");
}

function Card({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--surface)] p-4">
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h2 className="text-[13px] font-medium text-[var(--text)]">{title}</h2>
        {hint ? <span className="text-[11px] text-[var(--text-faint)]">{hint}</span> : null}
      </div>
      {children}
    </section>
  );
}

/** Full-window analytics view (replaces the canvas, not a drawer). */
export function AnalyticsPage() {
  const showCanvas = useUiStore((s) => s.showCanvas);
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
          setSamples((prev) => [...prev.slice(-179), a.resource]);
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

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        showCanvas();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showCanvas]);

  const cachePcts = useMemo(() => (data?.cache ?? []).map((p) => p.pct), [data]);
  const cacheMiss = useMemo(() => (data?.cache ?? []).map((p) => p.miss), [data]);
  const cpu = useMemo(() => samples.map((s) => s.cpu_pct), [samples]);
  const ram = useMemo(() => samples.map((s) => s.process_ram_mb), [samples]);
  const last = samples[samples.length - 1] ?? data?.resource;
  const maxTools = Math.max(1, ...(data?.tools_all.slice(0, 16).map((t) => t.count) ?? [1]));

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col bg-[var(--bg)]" aria-label="Analytics">
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--border)] px-6 py-3">
        <div>
          <h1 className="text-[16px] font-semibold text-[var(--text)]">Analytics</h1>
          <p className="text-[12px] text-[var(--text-faint)]">
            Cache, tools, and this app’s CPU / RAM / GPU. Charts are CSS/SVG only.
          </p>
        </div>
        <button
          type="button"
          className="rounded-[var(--radius-md)] border border-[var(--border)] px-3 py-1.5 text-[12px] text-[var(--text-dim)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
          onClick={showCanvas}
        >
          Back to canvas
        </button>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        {err ? <p className="mb-4 text-[13px] text-[var(--danger)]">{err}</p> : null}
        {!data ? (
          <p className="text-[13px] text-[var(--text-faint)]">Loading…</p>
        ) : (
          <div className="mx-auto flex max-w-[1200px] flex-col gap-5">
            <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
              <Stat
                label="Cache hit"
                value={`${data.cache_avg_pct.toFixed(0)}%`}
                hint={`${data.cache.length} recent model requests`}
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
                  last?.gpu_pct != null
                    ? `${last.gpu_pct.toFixed(0)}%`
                    : last?.gpu_name
                      ? "idle / shared"
                      : "n/a"
                }
                hint={gpuHint(last)}
              />
            </div>

            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <Card title="Cache hit rate" hint="recent turns, %">
                <Spark values={cachePcts} />
              </Card>
              <Card title="Uncached tokens (miss)" hint="lower is cheaper">
                <Spark values={cacheMiss} color="var(--warning)" />
              </Card>
              <Card title="App CPU" hint="while this page is open">
                <Spark values={cpu} />
              </Card>
              <Card title="App RAM (MB)" hint="this process">
                <Spark values={ram} />
              </Card>
            </div>

            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <Card title="Tools across recent chats">
                {data.tools_all.length === 0 ? (
                  <p className="text-[13px] text-[var(--text-faint)]">No tool history yet</p>
                ) : (
                  <ul className="flex flex-col gap-1.5">
                    {data.tools_all.slice(0, 16).map((t) => (
                      <li key={t.name} className="flex items-center gap-3">
                        <span className="w-40 shrink-0 truncate font-mono text-[12px] text-[var(--text-dim)]">
                          {t.name}
                        </span>
                        <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-[var(--bg)]">
                          <div
                            className="h-full rounded-full bg-[var(--accent)]"
                            style={{ width: `${Math.max(4, (t.count / maxTools) * 100)}%` }}
                          />
                        </div>
                        <span className="w-10 shrink-0 text-right font-mono text-[12px] text-[var(--text-faint)]">
                          {t.count}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </Card>

              <Card title="Sessions">
                {data.conversations.length === 0 ? (
                  <p className="text-[13px] text-[var(--text-faint)]">No chats saved yet</p>
                ) : (
                  <ul className="flex flex-col gap-2">
                    {data.conversations.map((c) => (
                      <li
                        key={c.id}
                        className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg)] px-3 py-2"
                      >
                        <div className="truncate text-[13px] text-[var(--text)]">
                          {c.title || "Untitled"}
                        </div>
                        <div className="mt-0.5 text-[12px] text-[var(--text-faint)]">
                          {c.user_turns} user turns · {c.tool_calls} tools
                          {c.updated_at ? ` · ${c.updated_at.slice(0, 16)}` : ""}
                        </div>
                        {c.tools[0] ? (
                          <div className="mt-0.5 font-mono text-[11px] text-[var(--text-faint)]">
                            top {c.tools[0].name} ×{c.tools[0].count}
                          </div>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                )}
              </Card>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
