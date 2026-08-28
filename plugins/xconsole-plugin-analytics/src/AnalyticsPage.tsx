import { useEffect, useMemo, useState, useCallback, type ReactNode } from "react";
import { api } from "./api";
import type { AgentAnalytics, ResourceSnapshot } from "./types";
import {
  ChartIcon,
  CacheIcon,
  ToolIcon,
  ChatIcon,
  CpuIcon,
  ServerIcon,
  DownloadIcon,
  RefreshIcon,
  PauseIcon,
  PlayIcon,
  CloseIcon,
} from "./icons";
import { DashboardTab } from "./components/DashboardTab";
import { CacheTelemetryTab } from "./components/CacheTelemetryTab";
import { ToolsTelemetryTab } from "./components/ToolsTelemetryTab";
import { ConversationsTab } from "./components/ConversationsTab";
import { HardwareMonitorTab } from "./components/HardwareMonitorTab";
import { InfrastructureTab } from "./components/InfrastructureTab";
import { ExportReportsTab } from "./components/ExportReportsTab";

type TabId =
  | "dashboard"
  | "cache"
  | "tools"
  | "sessions"
  | "hardware"
  | "infrastructure"
  | "export";

interface TabDef {
  id: TabId;
  label: string;
  icon: (props: { size?: number }) => ReactNode;
  badge?: string | number;
}

const TABS: TabDef[] = [
  { id: "dashboard", label: "Dashboard", icon: ChartIcon },
  { id: "cache", label: "AI Prompt Cache", icon: CacheIcon },
  { id: "tools", label: "Agent Tools", icon: ToolIcon },
  { id: "sessions", label: "Sessions", icon: ChatIcon },
  { id: "hardware", label: "Hardware & RAM", icon: CpuIcon },
  { id: "infrastructure", label: "Infrastructure", icon: ServerIcon },
  { id: "export", label: "Export & Reports", icon: DownloadIcon },
];

export function AnalyticsPage({ onClose }: { onClose?: () => void }) {
  const [activeTab, setActiveTab] = useState<TabId>("dashboard");
  const [data, setData] = useState<AgentAnalytics | null>(null);
  const [samples, setSamples] = useState<ResourceSnapshot[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [pollIntervalSec, setPollIntervalSec] = useState<number>(1); // 1s default for 60fps-smooth stream
  const [isPaused, setIsPaused] = useState(false);

  // 1. Fetch full analytics (cache, tools, sessions, resource)
  const fetchFullAnalytics = useCallback(async (showLoading = false) => {
    try {
      if (showLoading) setIsRefreshing(true);
      const a = await api.agentAnalytics();
      setData(a);
      setErr(null);
      setSamples((prev) => {
        const next = [...prev.slice(-119), a.resource];
        return next;
      });
    } catch (e: unknown) {
      setErr(String(e));
    } finally {
      if (showLoading) setIsRefreshing(false);
    }
  }, []);

  // 2. Fast lightweight resource tick (<0.2ms) for continuous smooth sliding window
  const fetchResourceTick = useCallback(async () => {
    if (document.hidden) return;
    try {
      const snap = await api.appResourceSnapshot();
      setSamples((prev) => {
        const next = [...prev.slice(-119), snap];
        return next;
      });
    } catch {
      // ignore transient tick failure
    }
  }, []);

  // Initial full fetch on mount & periodic 15s background refresh
  useEffect(() => {
    void fetchFullAnalytics(true);

    const fullInterval = window.setInterval(() => {
      if (!document.hidden && !isPaused) {
        void fetchFullAnalytics(false);
      }
    }, 15000);

    return () => window.clearInterval(fullInterval);
  }, [fetchFullAnalytics, isPaused]);

  // Fast real-time hardware stream polling loop
  useEffect(() => {
    if (isPaused || pollIntervalSec <= 0) return;

    const intervalId = window.setInterval(() => {
      void fetchResourceTick();
    }, pollIntervalSec * 1000);

    return () => {
      window.clearInterval(intervalId);
    };
  }, [fetchResourceTick, isPaused, pollIntervalSec]);

  // Handle ESC key to close modal
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const lastSample = useMemo(() => {
    return samples[samples.length - 1] ?? data?.resource;
  }, [samples, data]);

  return (
    <div
      className="flex h-full w-full min-h-0 flex-1 flex-col bg-[var(--surface,#09090b)] text-[var(--text,#fafafa)] select-none font-sans"
      aria-label="Analytics & Telemetry Suite"
    >
      {/* 1. Header Bar */}
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] px-6 py-3">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
            <ChartIcon size={18} />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-sm font-semibold tracking-tight text-white">
                Analytics &amp; Telemetry Suite
              </h1>
              <span className="rounded bg-cyan-500/10 border border-cyan-500/20 px-1.5 py-0.2 text-[10px] font-mono text-cyan-300">
                v2.0
              </span>
              <span className="flex items-center gap-1.5 text-[11px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse" />
                <span>Live Stream</span>
              </span>
            </div>
            <p className="text-[11px] text-[var(--text-faint,#71717a)]">
              AI prompt cache efficiency, agent tool execution intelligence, CPU/RAM/GPU telemetry
            </p>
          </div>
        </div>

        {/* Polling Rate & Controls */}
        <div className="flex items-center gap-2">
          {/* Polling Interval Select */}
          <div className="flex items-center rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-0.5 text-xs font-mono">
            <button
              type="button"
              onClick={() => setIsPaused(!isPaused)}
              className={`flex h-7 items-center gap-1 rounded px-2 transition ${
                isPaused
                  ? "bg-amber-500/20 text-amber-300"
                  : "text-[var(--text-dim,#a1a1aa)] hover:text-white"
              }`}
              title={isPaused ? "Resume real-time polling" : "Pause live polling"}
            >
              {isPaused ? <PlayIcon size={12} /> : <PauseIcon size={12} />}
              <span className="text-[10px]">{isPaused ? "Paused" : "Live"}</span>
            </button>

            <select
              value={pollIntervalSec}
              onChange={(e) => {
                setPollIntervalSec(Number(e.target.value));
                setIsPaused(false);
              }}
              disabled={isPaused}
              className="h-7 rounded bg-transparent px-2 text-[11px] text-[var(--text,#e4e4e7)] focus:outline-none cursor-pointer disabled:opacity-50"
            >
              <option value={1}>1s tick</option>
              <option value={3}>3s tick</option>
              <option value={5}>5s tick</option>
              <option value={10}>10s tick</option>
              <option value={30}>30s tick</option>
            </select>
          </div>

          {/* Manual Refresh Button */}
          <button
            type="button"
            onClick={() => void fetchFullAnalytics(true)}
            disabled={isRefreshing}
            className="flex h-8 items-center gap-1.5 rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] px-2.5 text-xs font-mono text-[var(--text-dim,#a1a1aa)] hover:bg-white/5 hover:text-white transition cursor-pointer disabled:opacity-50"
            title="Manual refresh"
          >
            <RefreshIcon
              size={13}
              className={isRefreshing ? "animate-spin text-cyan-400" : ""}
            />
            <span className="hidden sm:inline">Refresh</span>
          </button>

          {/* Close Button */}
          {onClose && (
            <button
              type="button"
              className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border,#27272a)] text-zinc-400 hover:bg-white/5 hover:text-white transition cursor-pointer"
              onClick={onClose}
              title="Close modal (Esc)"
            >
              <CloseIcon size={14} />
            </button>
          )}
        </div>
      </header>

      {/* 2. Category Tab Navigation Bar */}
      <div className="flex shrink-0 items-center gap-1.5 overflow-x-auto border-b border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)]/60 px-6 py-2">
        {TABS.map((tab) => {
          const isActive = activeTab === tab.id;
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition cursor-pointer border ${
                isActive
                  ? "border-cyan-500/40 bg-cyan-500/10 text-cyan-300 shadow-sm"
                  : "border-transparent text-[var(--text-dim,#a1a1aa)] hover:border-[var(--border,#27272a)] hover:bg-white/5 hover:text-white"
              }`}
            >
              <Icon size={14} />
              <span>{tab.label}</span>
            </button>
          );
        })}
      </div>

      {/* 3. Main Content Area */}
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 bg-[var(--surface,#09090b)]/40">
        {err ? (
          <div className="rounded-xl border border-red-500/30 bg-red-950/20 p-4 text-xs font-mono text-red-300">
            Error loading telemetry data: {err}
          </div>
        ) : null}

        {!data ? (
          <div className="flex h-64 flex-col items-center justify-center gap-3 text-xs text-[var(--text-faint,#71717a)]">
            <RefreshIcon size={24} className="animate-spin text-cyan-400" />
            <span>Connecting to xConsole telemetry kernel…</span>
          </div>
        ) : (
          <div className="mx-auto max-w-[1300px]">
            {activeTab === "dashboard" && (
              <DashboardTab
                data={data}
                samples={samples}
                onSelectTab={(tabId) => setActiveTab(tabId as TabId)}
              />
            )}

            {activeTab === "cache" && <CacheTelemetryTab data={data} />}

            {activeTab === "tools" && <ToolsTelemetryTab data={data} />}

            {activeTab === "sessions" && <ConversationsTab data={data} />}

            {activeTab === "hardware" && lastSample && (
              <HardwareMonitorTab samples={samples} lastSample={lastSample} />
            )}

            {activeTab === "infrastructure" && <InfrastructureTab />}

            {activeTab === "export" && <ExportReportsTab data={data} samples={samples} />}
          </div>
        )}
      </div>

      {/* 4. Footer Status Strip */}
      <footer className="flex shrink-0 items-center justify-between border-t border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] px-6 py-2 text-[11px] font-mono text-[var(--text-faint,#71717a)]">
        <div className="flex items-center gap-3">
          <span>Samples in buffer: {samples.length}</span>
          <span>&bull;</span>
          <span>
            Cache hit: <strong className="text-cyan-400">{data?.cache_avg_pct.toFixed(0) || 0}%</strong>
          </span>
          <span>&bull;</span>
          <span>
            Host CPU:{" "}
            <strong className="text-amber-400">
              {lastSample?.cpu_pct.toFixed(0) || 0}%
            </strong>
          </span>
        </div>

        <div className="flex items-center gap-2">
          <span>xConsole Cordis Telemetry</span>
        </div>
      </footer>
    </div>
  );
}

export default AnalyticsPage;
