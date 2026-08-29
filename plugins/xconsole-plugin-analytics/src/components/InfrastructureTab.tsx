import React, { useEffect, useState } from "react";
import { api } from "../api";
import type { Vps } from "../types";
import { useMaskHost } from "../../../../src/lib/privacy";
import {
  ServerIcon,
  FolderIcon,
  DatabaseIcon,
  CloudIcon,
  CheckCircleIcon,
} from "../icons";

export const InfrastructureTab = React.memo(function InfrastructureTab() {
  const [vpsList, setVpsList] = useState<Vps[]>([]);
  const [loading, setLoading] = useState(true);
  const maskHost = useMaskHost();

  useEffect(() => {
    let alive = true;
    api
      .listVps()
      .then((list: Vps[]) => {
        if (alive) {
          setVpsList(list);
          setLoading(false);
        }
      })
      .catch(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  return (
    <div className="flex flex-col gap-6 animate-in fade-in duration-200">
      {/* 1. Infrastructure Overview Cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-4">
        <div className="flex flex-col justify-between rounded-xl border border-cyan-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-cyan-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Configured VPS</span>
            <ServerIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-3xl font-bold text-cyan-300">
            {vpsList.length}
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            remote target nodes
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-emerald-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-emerald-400">
            <span className="text-xs font-semibold uppercase tracking-wider">SFTP Engine</span>
            <FolderIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-xl font-bold text-emerald-300">
            Active
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            multiplexed SSH transfer queue
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-indigo-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-indigo-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Database Engine</span>
            <DatabaseIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-xl font-bold text-indigo-300">
            Ready
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            MySQL, PostgreSQL, SQLite
          </div>
        </div>

        <div className="flex flex-col justify-between rounded-xl border border-amber-500/20 bg-[var(--surface,#09090b)] p-4">
          <div className="flex items-center justify-between text-amber-400">
            <span className="text-xs font-semibold uppercase tracking-wider">Cloudflare Tunnels</span>
            <CloudIcon size={18} />
          </div>
          <div className="mt-3 font-mono text-xl font-bold text-amber-300">
            Connected
          </div>
          <div className="mt-1 text-xs text-[var(--text-faint,#a1a1aa)]">
            Zero Trust &amp; DNS Management
          </div>
        </div>
      </div>

      {/* 2. VPS Nodes Directory */}
      <div className="rounded-xl border border-[var(--border,#27272a)] bg-[var(--surface,#09090b)] p-4">
        <div className="flex items-center justify-between border-b border-[var(--border,#27272a)] pb-3 mb-3">
          <div className="flex items-center gap-2">
            <ServerIcon size={16} className="text-cyan-400" />
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text,#e4e4e7)]">
              Managed Remote Hosts ({vpsList.length})
            </h3>
          </div>
          <span className="text-[11px] font-mono text-[var(--text-faint,#71717a)]">
            SSH / SFTP / MCP Target Matrix
          </span>
        </div>

        {loading ? (
          <div className="flex h-32 items-center justify-center text-xs text-[var(--text-faint,#71717a)]">
            Loading remote nodes…
          </div>
        ) : vpsList.length === 0 ? (
          <div className="flex h-32 flex-col items-center justify-center text-xs text-[var(--text-faint,#71717a)]">
            <ServerIcon size={24} className="mb-2 opacity-40" />
            No VPS servers configured yet in xConsole.
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {vpsList.map((srv) => (
              <div
                key={srv.id}
                className="flex flex-col justify-between rounded-lg border border-[var(--border,#27272a)] bg-[var(--surface-2,#18181b)] p-3.5 hover:border-cyan-500/30 transition-colors"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="flex h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
                    <span className="text-sm font-semibold text-white">{srv.name}</span>
                  </div>
                  <span className="rounded bg-cyan-500/10 border border-cyan-500/20 px-1.5 py-0.5 font-mono text-[10px] text-cyan-300">
                    Port {srv.port}
                  </span>
                </div>

                <div className="mt-3 flex flex-col gap-1 font-mono text-xs text-[var(--text-dim,#a1a1aa)]">
                  <div className="flex items-center justify-between">
                    <span className="text-[var(--text-faint,#71717a)]">Host:</span>
                    <span className="text-[var(--text,#fafafa)]">{maskHost(srv.host)}</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-[var(--text-faint,#71717a)]">User:</span>
                    <span className="text-[var(--text,#fafafa)]">{srv.username}</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-[var(--text-faint,#71717a)]">Auth:</span>
                    <span className="text-[var(--text,#fafafa)]">
                      {srv.auth_type === "key" ? "SSH Key" : "Password"}
                    </span>
                  </div>
                </div>

                <div className="mt-3 pt-2 border-t border-[var(--border,#27272a)] flex items-center justify-between text-[11px] text-[var(--text-faint,#71717a)]">
                  <span className="flex items-center gap-1 text-emerald-400">
                    <CheckCircleIcon size={12} />
                    <span>Live Target</span>
                  </span>
                  <span className="font-mono text-[10px]">ID: {srv.id.slice(0, 8)}...</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
});
