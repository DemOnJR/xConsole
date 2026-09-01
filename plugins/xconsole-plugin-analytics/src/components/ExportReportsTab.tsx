import React, { useState, useMemo } from "react";
import type { AgentAnalytics, ResourceSnapshot, ToolCount, CachePoint } from "../types";
import {
  CheckIcon,
  CopyIcon,
  DownloadIcon,
  InfoIcon,
} from "../../../../src/components/icons";

interface ExportReportsTabProps {
  data: AgentAnalytics;
  samples: ResourceSnapshot[];
}

export const ExportReportsTab = React.memo(function ExportReportsTab({ data, samples }: ExportReportsTabProps) {
  const [copied, setCopied] = useState(false);

  const jsonString = useMemo(() => {
    const fullReport = {
      exportedAt: new Date().toISOString(),
      version: "2.0.0",
      analytics: data,
      recentResourceSamples: samples,
    };
    return JSON.stringify(fullReport, null, 2);
  }, [data, samples]);

  const handleCopy = () => {
    void navigator.clipboard.writeText(jsonString);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownloadJSON = () => {
    const blob = new Blob([jsonString], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `xconsole-telemetry-${new Date().toISOString().slice(0, 19).replace(/:/g, "-")}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleDownloadToolsCSV = () => {
    const headers = "Tool Name,Execution Count,Percentage Share\n";
    const totalCalls = Math.max(1, data.tools_all.reduce((a: number, b: ToolCount) => a + b.count, 0));
    const rows = data.tools_all
      .map((t: ToolCount) => `"${t.name}",${t.count},${((t.count / totalCalls) * 100).toFixed(1)}%`)
      .join("\n");
    const blob = new Blob([headers + rows], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `xconsole-tools-analytics-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleDownloadCacheCSV = () => {
    const headers = "Timestamp,Session,Prompt Tokens,Hit Tokens,Miss Tokens,Hit Pct\n";
    const rows = data.cache
      .map((c: CachePoint) => `"${c.ts}","${c.session}",${c.prompt},${c.hit},${c.miss},${c.pct}%`)
      .join("\n");
    const blob = new Blob([headers + rows], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `xconsole-cache-analytics-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Export Action Cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="flex flex-col justify-between rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <div>
            <h4 className="text-sm font-semibold text-white">Full JSON Report</h4>
            <p className="mt-1 text-xs text-[var(--text-faint,#71717a)]">
              Complete raw state including hardware metrics, sessions, tools, and cache logs.
            </p>
          </div>
          <div className="mt-4 flex items-center gap-2">
            <button
              type="button"
              onClick={handleDownloadJSON}
              className="flex items-center gap-1.5 rounded-lg bg-cyan-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-cyan-500 transition cursor-pointer"
            >
              <DownloadIcon size={14} />
              <span>Download JSON</span>
            </button>
            <button
              type="button"
              onClick={handleCopy}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] px-3 py-1.5 text-xs text-[var(--text,#e4e4e7)] hover:bg-white/5 transition cursor-pointer"
            >
              {copied ? <CheckIcon size={14} className="text-emerald-400" /> : <CopyIcon size={14} />}
              <span>{copied ? "Copied!" : "Copy Raw"}</span>
            </button>
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <div>
            <h4 className="text-sm font-semibold text-white">Tool Invocations CSV</h4>
            <p className="mt-1 text-xs text-[var(--text-faint,#71717a)]">
              Structured spreadsheet containing all executed agent tool counts and usage ratios.
            </p>
          </div>
          <div className="mt-4">
            <button
              type="button"
              onClick={handleDownloadToolsCSV}
              className="flex items-center gap-1.5 rounded-lg border border-purple-500/40 bg-purple-500/10 px-3 py-1.5 text-xs font-medium text-purple-300 hover:bg-purple-500/20 transition cursor-pointer"
            >
              <DownloadIcon size={14} />
              <span>Export Tools (.csv)</span>
            </button>
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
          <div>
            <h4 className="text-sm font-semibold text-white">Prompt Cache CSV</h4>
            <p className="mt-1 text-xs text-[var(--text-faint,#71717a)]">
              Evaluation history of prompt cache hit vs miss tokens per turn.
            </p>
          </div>
          <div className="mt-4">
            <button
              type="button"
              onClick={handleDownloadCacheCSV}
              className="flex items-center gap-1.5 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-1.5 text-xs font-medium text-emerald-300 hover:bg-emerald-500/20 transition cursor-pointer"
            >
              <DownloadIcon size={14} />
              <span>Export Cache (.csv)</span>
            </button>
          </div>
        </div>
      </div>

      {/* 2. Raw JSON Preview */}
      <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
        <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-3 mb-3">
          <div className="flex items-center gap-2">
            <InfoIcon size={16} className="text-cyan-400" />
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              Raw Telemetry Payload Preview
            </h3>
          </div>
          <span className="font-mono text-[11px] text-[var(--text-faint,#71717a)]">
            {(jsonString.length / 1024).toFixed(1)} KB payload
          </span>
        </div>

        <pre className="max-h-96 overflow-y-auto rounded-lg bg-[var(--surface-2,#18181b)] p-4 font-mono text-xs text-[var(--text-dim,#a1a1aa)] select-text border border-[var(--border,#27272a)]">
          {jsonString}
        </pre>
      </div>
    </div>
  );
});
