import React, { useState, useMemo, useDeferredValue } from "react";
import type { AgentAnalytics, ToolCount } from "../types";
import {
  LayersIcon,
  SearchIcon,
  ZapIcon,
  ToolIcon,
} from "../icons";

interface ToolsTelemetryTabProps {
  data: AgentAnalytics;
}

function getToolCategory(name: string): { label: string; color: string; icon: string } {
  if (name.startsWith("db_") || name.includes("sql") || name.includes("database")) {
    return { label: "Database", color: "text-emerald-400 bg-emerald-500/10 border-emerald-500/20", icon: "db" };
  }
  if (name.startsWith("cloudflare_") || name.includes("tunnel") || name.includes("dns")) {
    return { label: "Cloudflare", color: "text-amber-400 bg-amber-500/10 border-amber-500/20", icon: "cf" };
  }
  if (name.includes("file") || name.includes("dir") || name.includes("sftp")) {
    return { label: "Filesystem", color: "text-blue-400 bg-blue-500/10 border-blue-500/20", icon: "file" };
  }
  if (name.includes("command") || name.includes("exec") || name.includes("run")) {
    return { label: "Terminal / Exec", color: "text-cyan-400 bg-cyan-500/10 border-cyan-500/20", icon: "term" };
  }
  if (name.includes("skill") || name.includes("memory") || name.includes("soul") || name.includes("taste")) {
    return { label: "Agent Memory", color: "text-purple-400 bg-purple-500/10 border-purple-500/20", icon: "mem" };
  }
  return { label: "General Tool", color: "text-zinc-400 bg-zinc-500/10 border-zinc-500/20", icon: "gen" };
}

export const ToolsTelemetryTab = React.memo(function ToolsTelemetryTab({ data }: ToolsTelemetryTabProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearch = useDeferredValue(searchQuery);
  const [selectedCategory, setSelectedCategory] = useState<string>("all");
  const [sortBy, setSortBy] = useState<"count" | "name">("count");

  const totalToolCalls = useMemo(
    () => data.tools_all.reduce((acc: number, t: ToolCount) => acc + t.count, 0),
    [data.tools_all],
  );

  const maxToolCount = useMemo(
    () => Math.max(1, ...(data.tools_all.map((t: ToolCount) => t.count) || [1])),
    [data.tools_all],
  );

  // Group by category counts
  const categoryStats = useMemo(() => {
    const map = new Map<string, number>();
    for (const t of data.tools_all) {
      const cat = getToolCategory(t.name).label;
      map.set(cat, (map.get(cat) || 0) + t.count);
    }
    return Array.from(map.entries()).map(([label, count]) => ({ label, count }));
  }, [data.tools_all]);

  // Filtered tools
  const filteredTools = useMemo(() => {
    const q = deferredSearch.toLowerCase().trim();
    return data.tools_all
      .filter((t: ToolCount) => {
        if (selectedCategory !== "all" && getToolCategory(t.name).label !== selectedCategory) {
          return false;
        }
        if (!q) return true;
        return t.name.toLowerCase().includes(q);
      })
      .sort((a: ToolCount, b: ToolCount) => {
        if (sortBy === "count") return b.count - a.count;
        return a.name.localeCompare(b.name);
      });
  }, [data.tools_all, deferredSearch, selectedCategory, sortBy]);

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Header Metrics */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="flex flex-col justify-between rounded-xl border border-purple-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-purple-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Total Invocations</span>
            <ToolIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-purple-300">
            {totalToolCalls.toLocaleString()}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            across all autonomous agent cycles
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-cyan-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-cyan-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Unique Tools</span>
            <LayersIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-cyan-300">
            {data.tools_all.length}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            registered &amp; executed tools
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-emerald-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-emerald-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Most Active Tool</span>
            <ZapIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-xl font-bold text-emerald-300 truncate">
            {data.tools_all[0]?.name || "None"}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            {data.tools_all[0]?.count || 0} executions (
            {Math.round(((data.tools_all[0]?.count || 0) / Math.max(1, totalToolCalls)) * 100)}% share)
          </div>
        </div>
      </div>

      {/* 2. Category Breakdown Pills */}
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => setSelectedCategory("all")}
          className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-mono transition border ${
            selectedCategory === "all"
              ? "bg-purple-500/20 text-purple-300 border-purple-500/40"
              : "bg-[var(--surface-2,#18181b)] text-[var(--text-dim,#a1a1aa)] border-[var(--border,#27272a)] hover:text-white"
          }`}
        >
          All Categories ({totalToolCalls})
        </button>

        {categoryStats.map((c) => (
          <button
            key={c.label}
            type="button"
            onClick={() => setSelectedCategory(c.label)}
            className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-mono transition border ${
              selectedCategory === c.label
                ? "bg-cyan-500/20 text-cyan-300 border-cyan-500/40"
                : "bg-[var(--surface-2,#18181b)] text-[var(--text-dim,#a1a1aa)] border-[var(--border,#27272a)] hover:text-white"
            }`}
          >
            <span>{c.label}</span>
            <span className="text-[10px] text-[var(--text-faint,#71717a)]">({c.count})</span>
          </button>
        ))}
      </div>

      {/* 3. Search and Matrix Table */}
      <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between border-b border-[var(--border,#27272a)] pb-3">
          <div className="flex items-center gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              Tool Matrix Directory ({filteredTools.length} tools)
            </h3>
          </div>

          <div className="flex items-center gap-2">
            {/* Search */}
            <div className="relative flex items-center">
              <div className="pointer-events-none absolute left-2.5 text-[var(--text-faint,#71717a)]">
                <SearchIcon size={14} />
              </div>
              <input
                type="text"
                placeholder="Search tool name..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="h-8 w-48 rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] pl-8 pr-2.5 text-xs text-[var(--text,#e4e4e7)] placeholder-[var(--text-faint,#71717a)] focus:border-purple-500 focus:outline-none"
              />
            </div>

            {/* Sort Toggle */}
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value as "count" | "name")}
              className="h-8 rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] px-2.5 text-xs text-[var(--text,#e4e4e7)] focus:border-purple-500 focus:outline-none cursor-pointer"
            >
              <option value="count">Sort by Calls (High &rarr; Low)</option>
              <option value="name">Sort by Name (A &rarr; Z)</option>
            </select>
          </div>
        </div>

        {filteredTools.length === 0 ? (
          <div className="flex h-36 items-center justify-center text-xs text-[var(--text-faint,#71717a)]">
            No tools found matching the search query.
          </div>
        ) : (
          <div className="mt-3 flex flex-col gap-3">
            {filteredTools.map((tool: ToolCount, idx: number) => {
              const cat = getToolCategory(tool.name);
              const pct = Math.round((tool.count / Math.max(1, totalToolCalls)) * 100);
              return (
                <div
                  key={tool.name}
                  className="rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)]/60 p-3 hover:border-purple-500/30 transition-colors"
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-3 min-w-0">
                      <span className="font-mono text-xs text-[var(--text-faint,#71717a)] w-6 shrink-0">
                        #{idx + 1}
                      </span>
                      <span className="font-mono text-sm font-semibold text-[var(--text,#fafafa)] truncate">
                        {tool.name}
                      </span>
                      <span
                        className={`rounded px-2 py-0.5 text-[10px] font-mono border uppercase tracking-wider ${cat.color}`}
                      >
                        {cat.label}
                      </span>
                    </div>

                    <div className="flex items-center gap-4 shrink-0 font-mono text-xs">
                      <span className="text-[var(--text-faint,#71717a)]">{pct}% share</span>
                      <span className="rounded bg-purple-500/10 border border-purple-500/20 px-2 py-0.5 font-bold text-purple-300">
                        {tool.count} calls
                      </span>
                    </div>
                  </div>

                  <div className="mt-2.5 h-1.5 w-full overflow-hidden rounded-full bg-[var(--surface,#09090b)]">
                    <div
                      className="h-full rounded-full bg-gradient-to-r from-purple-500 via-indigo-500 to-cyan-400 transition-all duration-500"
                      style={{ width: `${Math.max(2, (tool.count / maxToolCount) * 100)}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
});
