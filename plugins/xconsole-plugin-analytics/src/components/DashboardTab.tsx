import React, { useMemo } from "react";
import type { AgentAnalytics, ResourceSnapshot, CachePoint, ToolCount } from "../types";
import { AreaChart } from "./AreaChart";
import { Gauge } from "./Gauge";
import {
  CacheIcon,
  CpuIcon,
} from "../../../../src/components/icons";
import {
  ActivityIcon,
  ChatIcon,
  RamIcon,
  ToolIcon,
  TrendingUpIcon,
} from "../icons";

interface DashboardTabProps {
  data: AgentAnalytics;
  samples: ResourceSnapshot[];
  onSelectTab: (tabId: string) => void;
}

export const DashboardTab = React.memo(function DashboardTab({ data, samples, onSelectTab }: DashboardTabProps) {
  const lastSample = samples[samples.length - 1] ?? data.resource;

  // Cache sparkline numbers
  const cacheHitPercentages = useMemo(() => data.cache.map((c: CachePoint) => c.pct), [data.cache]);
  const cacheMissTokens = useMemo(() => data.cache.map((c: CachePoint) => c.miss), [data.cache]);
  const cacheTimestamps = useMemo(
    () => data.cache.map((c: CachePoint) => (c.ts ? c.ts.slice(11, 19) : "")),
    [data.cache],
  );

  // Hardware sliding window
  const ramValues = useMemo(() => samples.map((s: ResourceSnapshot) => s.process_ram_mb), [samples]);
  const sampleTimestamps = useMemo(
    () => samples.map((s: ResourceSnapshot) => (s.ts ? s.ts.slice(11, 19) : "")),
    [samples],
  );

  // Top tools
  const topTools = useMemo(() => data.tools_all.slice(0, 6), [data.tools_all]);
  const maxToolCount = useMemo(
    () => Math.max(1, ...(topTools.map((t: ToolCount) => t.count) || [1])),
    [topTools],
  );

  // Total tool calls across all chats
  const totalToolCalls = useMemo(
    () => data.tools_all.reduce((acc: number, t: ToolCount) => acc + t.count, 0),
    [data.tools_all],
  );

  // Estimated tokens saved via caching
  const totalCachedTokens = useMemo(
    () => data.cache.reduce((acc: number, c: CachePoint) => acc + c.hit, 0),
    [data.cache],
  );

  const systemRamPct = lastSample.ram_total_mb
    ? (lastSample.ram_mb / lastSample.ram_total_mb) * 100
    : 0;

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Top Executive KPI Cards */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
        {/* Cache Hit Rate */}
        <div className="flex flex-col justify-between rounded-xl border border-cyan-500/20 bg-gradient-to-br from-cyan-950/20 via-[var(--surface-2,#18181b)] to-[var(--surface,#09090b)] p-3.5 shadow-sm">
          <div className="flex items-center justify-between text-cyan-400">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-cyan-400/80">
              Cache Hit Rate
            </span>
            <CacheIcon size={16} />
          </div>
          <div className="mt-2 font-mono text-2xl font-bold text-cyan-300">
            {data.cache_avg_pct.toFixed(0)}%
          </div>
          <div className="mt-1 flex items-center gap-1 text-[11px] text-[var(--text-faint,#a1a1aa)]">
            <span>{data.cache.length} model turns</span>
          </div>
        </div>

        {/* Process RAM */}
        <div className="flex flex-col justify-between rounded-xl border border-indigo-500/20 bg-gradient-to-br from-indigo-950/20 via-[var(--surface-2,#18181b)] to-[var(--surface,#09090b)] p-3.5 shadow-sm">
          <div className="flex items-center justify-between text-indigo-400">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-indigo-400/80">
              App Process RAM
            </span>
            <RamIcon size={16} />
          </div>
          <div className="mt-2 font-mono text-2xl font-bold text-indigo-300">
            {lastSample.process_ram_mb} <span className="text-xs font-normal">MB</span>
          </div>
          <div className="mt-1 text-[11px] text-[var(--text-faint,#a1a1aa)]">
            xConsole Core &amp; Webview
          </div>
        </div>

        {/* System RAM */}
        <div className="flex flex-col justify-between rounded-xl border border-emerald-500/20 bg-gradient-to-br from-emerald-950/20 via-[var(--surface-2,#18181b)] to-[var(--surface,#09090b)] p-3.5 shadow-sm">
          <div className="flex items-center justify-between text-emerald-400">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-emerald-400/80">
              System RAM
            </span>
            <RamIcon size={16} />
          </div>
          <div className="mt-2 font-mono text-2xl font-bold text-emerald-300">
            {Math.round(systemRamPct)}%
          </div>
          <div className="mt-1 text-[11px] text-[var(--text-faint,#a1a1aa)] truncate">
            {lastSample.ram_mb} / {lastSample.ram_total_mb} MB
          </div>
        </div>

        {/* Host CPU */}
        <div className="flex flex-col justify-between rounded-xl border border-amber-500/20 bg-gradient-to-br from-amber-950/20 via-[var(--surface-2,#18181b)] to-[var(--surface,#09090b)] p-3.5 shadow-sm">
          <div className="flex items-center justify-between text-amber-400">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-amber-400/80">
              Host CPU Load
            </span>
            <CpuIcon size={16} />
          </div>
          <div className="mt-2 font-mono text-2xl font-bold text-amber-300">
            {lastSample.cpu_pct.toFixed(0)}%
          </div>
          <div className="mt-1 text-[11px] text-[var(--text-faint,#a1a1aa)]">
            {samples.length} live samples
          </div>
        </div>

        {/* Tool Invocations */}
        <div className="flex flex-col justify-between rounded-xl border border-purple-500/20 bg-gradient-to-br from-purple-950/20 via-[var(--surface-2,#18181b)] to-[var(--surface,#09090b)] p-3.5 shadow-sm">
          <div className="flex items-center justify-between text-purple-400">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-purple-400/80">
              Tool Calls
            </span>
            <ToolIcon size={16} />
          </div>
          <div className="mt-2 font-mono text-2xl font-bold text-purple-300">
            {totalToolCalls}
          </div>
          <div className="mt-1 text-[11px] text-[var(--text-faint,#a1a1aa)]">
            across {data.tools_all.length} distinct tools
          </div>
        </div>

        {/* Conversations */}
        <div className="flex flex-col justify-between rounded-xl border border-rose-500/20 bg-gradient-to-br from-rose-950/20 via-[var(--surface-2,#18181b)] to-[var(--surface,#09090b)] p-3.5 shadow-sm">
          <div className="flex items-center justify-between text-rose-400">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-rose-400/80">
              Agent Sessions
            </span>
            <ChatIcon size={16} />
          </div>
          <div className="mt-2 font-mono text-2xl font-bold text-rose-300">
            {data.conversations.length}
          </div>
          <div className="mt-1 text-[11px] text-[var(--text-faint,#a1a1aa)]">
            saved conversations
          </div>
        </div>
      </div>

      {/* 2. Radial Gauges & Live Stream Overview */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Hardware Radial Gauges */}
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4 flex flex-col justify-between">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-2.5">
            <div className="flex items-center gap-2">
              <ActivityIcon size={15} className="text-cyan-400" />
              <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
                Resource Utilization
              </h3>
            </div>
            <button
              type="button"
              onClick={() => onSelectTab("hardware")}
              className="text-[11px] text-cyan-400 hover:text-cyan-300 transition"
            >
              Hardware details &rarr;
            </button>
          </div>

          <div className="grid grid-cols-3 gap-2 my-auto py-3">
            <Gauge
              value={lastSample.cpu_pct}
              label="CPU"
              sublabel={`${lastSample.cpu_pct.toFixed(0)}%`}
              size={90}
              strokeWidth={7}
            />
            <Gauge
              value={systemRamPct}
              label="System RAM"
              sublabel={`${Math.round(systemRamPct)}%`}
              size={90}
              strokeWidth={7}
            />
            <Gauge
              value={data.cache_avg_pct}
              label="AI Cache"
              sublabel={`${data.cache_avg_pct.toFixed(0)}%`}
              size={90}
              strokeWidth={7}
              color="#06b6d4"
            />
          </div>

          <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-2.5 text-[11px] text-[var(--text-dim,#a1a1aa)] flex items-center justify-between">
            <span>GPU: {lastSample.gpu_name || "Intel / Integrated"}</span>
            <span className="font-mono text-cyan-400">
              {lastSample.gpu_pct != null ? `${lastSample.gpu_pct}% load` : "Active"}
            </span>
          </div>
        </div>

        {/* Live CPU & RAM Trend Chart */}
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4 flex flex-col justify-between lg:col-span-2">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-2.5 mb-2">
            <div className="flex items-center gap-2">
              <TrendingUpIcon size={15} className="text-indigo-400" />
              <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
                Real-Time Memory &amp; CPU Stream
              </h3>
            </div>
            <span className="text-[11px] font-mono text-[var(--text-faint,#71717a)]">
              Last {samples.length} ticks (sliding window)
            </span>
          </div>

          <AreaChart
            data={ramValues}
            labels={sampleTimestamps}
            height={130}
            color="#818cf8"
            fillColor="#6366f1"
            unit="MB"
            title="xConsole Process Memory"
            subtitle="Live Resident Set Size (RSS) in RAM"
          />
        </div>
      </div>

      {/* 3. AI Cache Performance & Agent Tools Section */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        {/* Cache Hit Distribution Chart */}
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4 flex flex-col justify-between">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-2.5 mb-2">
            <div className="flex items-center gap-2">
              <CacheIcon size={15} className="text-cyan-400" />
              <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
                AI Prompt Cache Hit Ratio (%)
              </h3>
            </div>
            <button
              type="button"
              onClick={() => onSelectTab("cache")}
              className="text-[11px] text-cyan-400 hover:text-cyan-300 transition"
            >
              Cache logs &rarr;
            </button>
          </div>

          <AreaChart
            data={cacheHitPercentages}
            labels={cacheTimestamps}
            height={140}
            color="#06b6d4"
            fillColor="#0891b2"
            unit="%"
            title="Token Cache Hit Efficiency"
            subtitle="Higher is better (minimizes LLM inference cost and latency)"
            valueFormatter={(v) => `${Math.round(v)}% hit`}
          />

          <div className="mt-3 grid grid-cols-2 gap-2 text-xs">
            <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-2 border border-[var(--border,#27272a)]">
              <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">
                Total Cached Tokens
              </div>
              <div className="font-mono text-base font-bold text-cyan-300">
                {totalCachedTokens.toLocaleString()}
              </div>
            </div>
            <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-2 border border-[var(--border,#27272a)]">
              <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">
                Uncached Miss Tokens
              </div>
              <div className="font-mono text-base font-bold text-amber-300">
                {cacheMissTokens.reduce((a: number, b: number) => a + b, 0).toLocaleString()}
              </div>
            </div>
          </div>
        </div>

        {/* Top Tools Execution Distribution */}
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4 flex flex-col justify-between">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-2.5 mb-2">
            <div className="flex items-center gap-2">
              <ToolIcon size={15} className="text-purple-400" />
              <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
                Top Executed Agent Tools
              </h3>
            </div>
            <button
              type="button"
              onClick={() => onSelectTab("tools")}
              className="text-[11px] text-purple-400 hover:text-purple-300 transition"
            >
              Full matrix &rarr;
            </button>
          </div>

          {topTools.length === 0 ? (
            <div className="flex h-44 items-center justify-center text-xs text-[var(--text-faint,#71717a)]">
              No tool executions logged yet. Run commands or agent queries to populate.
            </div>
          ) : (
            <div className="flex flex-col gap-2.5 my-auto">
              {topTools.map((t: ToolCount, idx: number) => {
                const pct = Math.round((t.count / Math.max(1, totalToolCalls)) * 100);
                return (
                  <div key={t.name} className="flex flex-col gap-1">
                    <div className="flex items-center justify-between text-xs font-mono">
                      <span className="flex items-center gap-2 text-[var(--text,#e4e4e7)]">
                        <span className="text-[10px] text-[var(--text-faint,#71717a)]">
                          #{idx + 1}
                        </span>
                        <strong>{t.name}</strong>
                      </span>
                      <span className="text-[var(--text-dim,#a1a1aa)]">
                        {t.count} calls ({pct}%)
                      </span>
                    </div>
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--surface-2,#18181b)]">
                      <div
                        className="h-full rounded-full bg-gradient-to-r from-purple-500 to-cyan-500 transition-all duration-500"
                        style={{ width: `${Math.max(4, (t.count / maxToolCount) * 100)}%` }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          <div className="mt-3 flex items-center justify-between text-[11px] text-[var(--text-faint,#71717a)]">
            <span>Aggregated across all autonomous sessions</span>
            <span className="font-mono text-purple-400 font-semibold">{totalToolCalls} total</span>
          </div>
        </div>
      </div>
    </div>
  );
});
