import React, { useState, useMemo, useDeferredValue } from "react";
import type { AgentAnalytics, CachePoint } from "../types";
import { AreaChart } from "./AreaChart";
import {
  CacheIcon,
  SearchIcon,
  ZapIcon,
} from "../../../../src/components/icons";
import {
  AlertTriangleIcon,
  TrendingUpIcon,
} from "../icons";

interface CacheTelemetryTabProps {
  data: AgentAnalytics;
}

export const CacheTelemetryTab = React.memo(function CacheTelemetryTab({ data }: CacheTelemetryTabProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearch = useDeferredValue(searchQuery);
  const [minPctFilter, setMinPctFilter] = useState<number>(0);

  // Compute metrics
  const totalCachedTokens = useMemo(
    () => data.cache.reduce((acc: number, c: CachePoint) => acc + c.hit, 0),
    [data.cache],
  );
  const totalMissTokens = useMemo(
    () => data.cache.reduce((acc: number, c: CachePoint) => acc + c.miss, 0),
    [data.cache],
  );

  // Estimated dollar savings ($0.15 per 1M tokens saved for prompt cache)
  const estimatedSavings = useMemo(() => {
    return ((totalCachedTokens / 1_000_000) * 0.15).toFixed(3);
  }, [totalCachedTokens]);

  // Chart data
  const hitPercentages = useMemo(() => data.cache.map((c: CachePoint) => c.pct), [data.cache]);
  const hitTokens = useMemo(() => data.cache.map((c: CachePoint) => c.hit), [data.cache]);
  const timestamps = useMemo(
    () => data.cache.map((c: CachePoint) => (c.ts ? c.ts.slice(11, 19) : "")),
    [data.cache],
  );

  // Filtered log table
  const filteredLog = useMemo(() => {
    const q = deferredSearch.toLowerCase().trim();
    return data.cache
      .filter((item: CachePoint) => {
        if (item.pct < minPctFilter) return false;
        if (!q) return true;
        return (
          item.session.toLowerCase().includes(q) ||
          item.ts.toLowerCase().includes(q) ||
          String(item.prompt).includes(q)
        );
      })
      .slice()
      .reverse(); // Newest first
  }, [data.cache, deferredSearch, minPctFilter]);

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Cache Header KPIs */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <div className="flex flex-col justify-between rounded-xl border border-cyan-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-cyan-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Average Hit Rate</span>
            <CacheIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-cyan-300">
            {data.cache_avg_pct.toFixed(1)}%
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            across {data.cache.length} model queries
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-emerald-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-emerald-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Cached Tokens</span>
            <ZapIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-emerald-300">
            {totalCachedTokens.toLocaleString()}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            tokens served from local cache
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-amber-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-amber-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Uncached Tokens</span>
            <AlertTriangleIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-amber-300">
            {totalMissTokens.toLocaleString()}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            processed as full misses
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-indigo-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-indigo-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Estimated Savings</span>
            <TrendingUpIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-indigo-300">
            ${estimatedSavings}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            based on standard token pricing
          </div>
        </div>
      </div>

      {/* 2. Visual Charts (Hit Rate % & Volume) */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
            Prompt Cache Hit Efficiency (%)
          </h3>
          <AreaChart
            data={hitPercentages}
            labels={timestamps}
            height={150}
            color="#06b6d4"
            fillColor="#0891b2"
            unit="%"
            title="Hit Rate Timeline"
            subtitle="Percentage of prompt tokens matched in previous prefix"
            valueFormatter={(v) => `${Math.round(v)}%`}
          />
        </div>

        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
            Cached vs Uncached Tokens Volume
          </h3>
          <AreaChart
            data={hitTokens}
            labels={timestamps}
            height={150}
            color="#10b981"
            fillColor="#059669"
            unit="tokens"
            title="Cached Token Volume (Hit)"
            subtitle="Tokens re-used instantly without extra computation"
          />
        </div>
      </div>

      {/* 3. Detailed Cache Logs Table */}
      <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between border-b border-[var(--border,#27272a)] pb-3">
          <div>
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              Cache Inspection Stream ({filteredLog.length} records)
            </h3>
            <p className="text-[11px] text-[var(--text-faint,#71717a)]">
              Detailed breakdown of prompt cache evaluation per agent turn
            </p>
          </div>

          <div className="flex items-center gap-2">
            {/* Search */}
            <div className="relative flex items-center">
              <div className="pointer-events-none absolute left-2.5 text-[var(--text-faint,#71717a)]">
                <SearchIcon size={14} />
              </div>
              <input
                type="text"
                placeholder="Search session ID / time..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="h-8 w-48 rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] pl-8 pr-2.5 text-xs text-[var(--text,#e4e4e7)] placeholder-[var(--text-faint,#71717a)] focus:border-cyan-500 focus:outline-none"
              />
            </div>

            {/* Filter */}
            <select
              value={minPctFilter}
              onChange={(e) => setMinPctFilter(Number(e.target.value))}
              className="h-8 rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] px-2.5 text-xs text-[var(--text,#e4e4e7)] focus:border-cyan-500 focus:outline-none cursor-pointer"
            >
              <option value={0}>All Hit Rates</option>
              <option value={50}>&ge; 50% Hit</option>
              <option value={80}>&ge; 80% Hit</option>
              <option value={95}>&ge; 95% High Hit</option>
            </select>
          </div>
        </div>

        {filteredLog.length === 0 ? (
          <div className="flex h-36 items-center justify-center text-xs text-[var(--text-faint,#71717a)]">
            No cache records found matching current criteria.
          </div>
        ) : (
          <div className="mt-3 max-h-80 overflow-y-auto">
            <table className="w-full text-left text-xs font-mono">
              <thead className="sticky top-0 bg-[var(--surface-2,#18181b)] text-[10px] uppercase text-[var(--text-faint,#71717a)] border-b border-[var(--border,#27272a)]">
                <tr>
                  <th className="py-2 px-3">Timestamp</th>
                  <th className="py-2 px-3">Session</th>
                  <th className="py-2 px-3 text-right">Total Prompt</th>
                  <th className="py-2 px-3 text-right">Hit Tokens</th>
                  <th className="py-2 px-3 text-right">Miss Tokens</th>
                  <th className="py-2 px-3 text-right">Efficiency</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border,#27272a)]/40">
                {filteredLog.map((row: CachePoint, idx: number) => {
                  const hitColor =
                    row.pct >= 80
                      ? "text-emerald-400 bg-emerald-500/10 border-emerald-500/20"
                      : row.pct >= 50
                        ? "text-cyan-400 bg-cyan-500/10 border-cyan-500/20"
                        : "text-amber-400 bg-amber-500/10 border-amber-500/20";
                  return (
                    <tr key={`${row.ts}-${idx}`} className="hover:bg-white/5 transition-colors">
                      <td className="py-2 px-3 text-[var(--text-dim,#a1a1aa)]">
                        {row.ts ? row.ts.replace("T", " ").slice(0, 19) : "—"}
                      </td>
                      <td className="py-2 px-3 text-[var(--text,#e4e4e7)] truncate max-w-[140px]">
                        {row.session || "global"}
                      </td>
                      <td className="py-2 px-3 text-right text-[var(--text-dim,#a1a1aa)]">
                        {row.prompt.toLocaleString()}
                      </td>
                      <td className="py-2 px-3 text-right font-semibold text-emerald-400">
                        {row.hit.toLocaleString()}
                      </td>
                      <td className="py-2 px-3 text-right text-amber-400">
                        {row.miss.toLocaleString()}
                      </td>
                      <td className="py-2 px-3 text-right">
                        <span
                          className={`inline-block rounded px-2 py-0.5 text-[10px] font-bold border ${hitColor}`}
                        >
                          {row.pct}%
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
});
