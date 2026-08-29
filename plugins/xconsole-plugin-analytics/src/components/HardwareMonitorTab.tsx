import React, { useMemo } from "react";
import type { ResourceSnapshot } from "../types";
import { AreaChart } from "./AreaChart";
import { Gauge } from "./Gauge";
import {
  CpuIcon,
  ServerIcon,
  GpuIcon,
  RamIcon,
} from "../icons";

interface HardwareMonitorTabProps {
  samples: ResourceSnapshot[];
  lastSample: ResourceSnapshot;
}

export const HardwareMonitorTab = React.memo(function HardwareMonitorTab({ samples, lastSample }: HardwareMonitorTabProps) {
  const cpuData = useMemo(() => samples.map((s: ResourceSnapshot) => s.cpu_pct), [samples]);
  const systemRamData = useMemo(() => samples.map((s: ResourceSnapshot) => s.ram_mb), [samples]);
  const timestamps = useMemo(
    () => samples.map((s: ResourceSnapshot) => (s.ts ? s.ts.slice(11, 19) : "")),
    [samples],
  );

  const systemRamPct = lastSample.ram_total_mb
    ? (lastSample.ram_mb / lastSample.ram_total_mb) * 100
    : 0;

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Radial Gauge HUD */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Gauge
          value={lastSample.cpu_pct}
          label="Host CPU Load"
          sublabel="Current CPU utilization"
          size={110}
          strokeWidth={8}
          icon={<CpuIcon size={18} />}
        />
        <Gauge
          value={systemRamPct}
          label="System RAM"
          sublabel={`${lastSample.ram_mb} / ${lastSample.ram_total_mb} MB`}
          size={110}
          strokeWidth={8}
          icon={<RamIcon size={18} />}
        />
        <Gauge
          value={lastSample.process_ram_mb > 0 ? Math.min(100, (lastSample.process_ram_mb / 2048) * 100) : 0}
          label="Process RAM"
          sublabel={`${lastSample.process_ram_mb} MB RSS`}
          size={110}
          strokeWidth={8}
          icon={<ServerIcon size={18} />}
          color="#818cf8"
          formatValue={() => `${lastSample.process_ram_mb} MB`}
        />
        <Gauge
          value={lastSample.gpu_pct ?? (lastSample.gpu_name ? 10 : 0)}
          label="GPU Accelerator"
          sublabel={lastSample.gpu_name || "Integrated / Shared"}
          size={110}
          strokeWidth={8}
          icon={<GpuIcon size={18} />}
          color="#10b981"
          formatValue={(v) => (lastSample.gpu_pct != null ? `${v}%` : "Online")}
        />
      </div>

      {/* 2. Detailed Hardware Info Banner */}
      <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
        <div className="flex items-center gap-2 border-b border-[var(--border,#27272a)] pb-3 mb-3">
          <GpuIcon size={16} className="text-emerald-400" />
          <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
            GPU &amp; Video Memory Telemetry
          </h3>
        </div>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3 text-xs font-mono">
          <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-3 border border-[var(--border,#27272a)]">
            <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">Device Name</div>
            <div className="mt-1 font-semibold text-white truncate">
              {lastSample.gpu_name || "Default Display Adapter / Intel UHD"}
            </div>
          </div>

          <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-3 border border-[var(--border,#27272a)]">
            <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">Dedicated VRAM</div>
            <div className="mt-1 font-semibold text-emerald-400">
              {lastSample.gpu_mem_total_mb
                ? `${lastSample.gpu_mem_total_mb} MB`
                : "Shared System Memory"}
            </div>
          </div>

          <div className="rounded-lg bg-[var(--surface-2,#18181b)] p-3 border border-[var(--border,#27272a)]">
            <div className="text-[10px] uppercase text-[var(--text-faint,#71717a)]">VRAM in Use</div>
            <div className="mt-1 font-semibold text-cyan-400">
              {lastSample.gpu_mem_mb ? `${lastSample.gpu_mem_mb} MB` : "Dynamic Allocation"}
            </div>
          </div>
        </div>
      </div>

      {/* 3. Live Historical Streams */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-2.5 mb-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              Host CPU Usage (%)
            </h3>
            <span className="font-mono text-[11px] text-amber-400">
              Latest: {lastSample.cpu_pct.toFixed(1)}%
            </span>
          </div>
          <AreaChart
            data={cpuData}
            labels={timestamps}
            height={150}
            color="#f59e0b"
            fillColor="#d97706"
            unit="%"
            title="CPU Load History"
            subtitle="Sliding window of process & system CPU load"
            valueFormatter={(v) => `${v.toFixed(1)}%`}
          />
        </div>

        <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-2.5 mb-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              System RAM Allocation (MB)
            </h3>
            <span className="font-mono text-[11px] text-emerald-400">
              Latest: {lastSample.ram_mb} MB
            </span>
          </div>
          <AreaChart
            data={systemRamData}
            labels={timestamps}
            height={150}
            color="#10b981"
            fillColor="#059669"
            unit="MB"
            title="Host RAM Allocation"
            subtitle={`Total host capacity: ${lastSample.ram_total_mb} MB`}
            valueFormatter={(v) => `${Math.round(v)} MB`}
          />
        </div>
      </div>
    </div>
  );
});
