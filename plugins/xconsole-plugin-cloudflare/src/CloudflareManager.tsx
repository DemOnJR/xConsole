import { useEffect, useState } from "react";
import { useCloudflareStore } from "./store";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { CloudflareDnsRecordInput, CloudflareIngressRule } from "./types";
import { Button, Field, Select, TextInput } from "./ui";
import {
  CloudIcon,
  ShieldIcon,
  GlobeIcon,
  HistoryIcon,
  PlusIcon,
  TrashIcon,
  CopyIcon,
  CheckIcon,
  RefreshIcon,
  CloseIcon,
  SearchIcon,
} from "./icons";

export function CloudflareManager({ onClose }: { onClose?: () => void }) {
  const {
    accounts,
    selectedAccountId,
    zones,
    selectedZoneId,
    tunnels,
    selectedTunnel,
    tunnelConfig,
    tunnelToken,
    dnsRecords,
    error,
    history,
    loadAccounts,
    selectAccount,
    selectZone,
    selectTunnel,
    createTunnel,
    deleteTunnel,
    saveTunnelConfig,
    upsertDnsRecord,
    deleteDnsRecord,
    toggleUnderAttackMode,
    loadHistory,
    revertAction,
  } = useCloudflareStore();

  const [activeTab, setActiveTab] = useState<"tunnels" | "dns" | "security" | "history">("tunnels");
  const [revertingId, setRevertingId] = useState<string | null>(null);
  const [revertToast, setRevertToast] = useState<string | null>(null);
  const [creatingTunnel, setCreatingTunnel] = useState(false);
  const [newTunnelName, setNewTunnelName] = useState("");
  const [addingIngress, setAddingIngress] = useState(false);
  const [ingressForm, setIngressForm] = useState<CloudflareIngressRule>({
    hostname: "",
    path: "",
    service: "http://localhost:3000",
  });

  // DNS modal state
  const [editingDns, setEditingDns] = useState<CloudflareDnsRecordInput | null>(null);
  const [dnsSearch, setDnsSearch] = useState("");
  const [dnsTypeFilter, setDnsTypeFilter] = useState("ALL");

  const [copiedToken, setCopiedToken] = useState(false);
  const [showSetupGuide, setShowSetupGuide] = useState(false);
  const [tokenInput, setTokenInput] = useState("");
  const [savingToken, setSavingToken] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);

  useEffect(() => {
    loadAccounts();
  }, [loadAccounts]);

  const handleCreateTunnel = async () => {
    if (!newTunnelName.trim()) return;
    try {
      await createTunnel(newTunnelName.trim());
      setNewTunnelName("");
      setCreatingTunnel(false);
    } catch (e) {
      alert(`Eroare la creare tunel: ${e}`);
    }
  };

  const handleAddIngress = async () => {
    if (!ingressForm.service.trim() || !tunnelConfig) return;
    try {
      const currentRules = tunnelConfig.ingress.filter(
        (r) => r.hostname || !r.service.startsWith("http_status:")
      );
      const newRules = [
        ...currentRules,
        {
          hostname: ingressForm.hostname?.trim() || undefined,
          path: ingressForm.path?.trim() || undefined,
          service: ingressForm.service.trim(),
        },
      ];
      await saveTunnelConfig({ ingress: newRules });
      setAddingIngress(false);
      setIngressForm({ hostname: "", path: "", service: "http://localhost:3000" });
    } catch (e) {
      alert(`Eroare la salvare rută: ${e}`);
    }
  };

  const handleRemoveIngress = async (index: number) => {
    if (!tunnelConfig) return;
    try {
      const newRules = tunnelConfig.ingress.filter((_, idx) => idx !== index);
      await saveTunnelConfig({ ingress: newRules });
    } catch (e) {
      alert(`Eroare la ștergere rută: ${e}`);
    }
  };

  const handleSaveDns = async () => {
    if (!editingDns) return;
    try {
      await upsertDnsRecord(editingDns);
      setEditingDns(null);
    } catch (e) {
      alert(`Eroare la salvare DNS: ${e}`);
    }
  };

  const filteredDns = dnsRecords.filter((rec) => {
    const matchesSearch =
      dnsSearch === "" ||
      rec.name.toLowerCase().includes(dnsSearch.toLowerCase()) ||
      rec.content.toLowerCase().includes(dnsSearch.toLowerCase());
    const matchesType = dnsTypeFilter === "ALL" || rec.type === dnsTypeFilter;
    return matchesSearch && matchesType;
  });

  const handleSaveManualToken = async () => {
    if (!tokenInput.trim()) return;
    setSavingToken(true);
    setTokenError(null);
    try {
      await invoke("save_cloudflare_manual_token", { token: tokenInput.trim() });
      await loadAccounts();
      setTokenInput("");
      setShowSetupGuide(false);
    } catch (e) {
      setTokenError(String(e));
    } finally {
      setSavingToken(false);
    }
  };

  const handleRevert = async (logId: string) => {
    if (!confirm("Sigur vrei să anulezi această modificare și să restaurezi starea anterioară în Cloudflare?")) {
      return;
    }
    setRevertingId(logId);
    setRevertToast(null);
    try {
      const msg = await revertAction(logId);
      setRevertToast(msg);
      setTimeout(() => setRevertToast(null), 5000);
    } catch (e) {
      alert(`Eroare la rollback: ${e}`);
    } finally {
      setRevertingId(null);
    }
  };

  return (
    <div className="flex h-full flex-col bg-[var(--surface)] text-[var(--text)] select-none">
      {/* Top Application Header */}
      <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5 bg-[var(--surface-2)]">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <div className="flex h-7 w-7 items-center justify-center rounded-md bg-orange-500/10 border border-orange-500/20 text-orange-400">
              <CloudIcon size={16} />
            </div>
            <div>
              <h2 className="text-xs font-semibold tracking-wide text-gray-100 uppercase">
                Cloudflare Zero Trust
              </h2>
            </div>
          </div>

          {accounts.length > 0 && (
            <div className="flex items-center gap-1.5 ml-2 pl-3 border-l border-[var(--border)]">
              <Select
                value={selectedAccountId ?? ""}
                onChange={(e) => selectAccount(e.target.value)}
                className="text-xs py-1 h-7 bg-[var(--bg)] border-[var(--border)]"
              >
                {accounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </Select>

              {zones.length > 0 && (
                <Select
                  value={selectedZoneId ?? ""}
                  onChange={(e) => selectZone(e.target.value)}
                  className="text-xs py-1 h-7 bg-[var(--bg)] border-[var(--border)]"
                >
                  {zones.map((z) => (
                    <option key={z.id} value={z.id}>
                      {z.name}
                    </option>
                  ))}
                </Select>
              )}

              <Button
                variant="ghost"
                className="text-[11px] text-gray-400 hover:text-red-400 hover:bg-red-950/20 px-2 py-1 h-7"
                title="Șterge contul Cloudflare selectat din xConsole"
                onClick={async () => {
                  if (selectedAccountId && confirm("Sigur vrei să deconectezi acest cont Cloudflare?")) {
                    await invoke("delete_cloud_account", { id: selectedAccountId });
                    await loadAccounts();
                  }
                }}
              >
                <TrashIcon size={12} />
              </Button>
            </div>
          )}
        </div>

        <div className="flex items-center gap-2">
          {accounts.length > 0 && (
            <button
              type="button"
              className={`flex items-center gap-1.5 rounded border px-2.5 py-1 text-xs transition ${
                showSetupGuide
                  ? "border-orange-500/40 bg-orange-500/10 text-orange-300"
                  : "border-[var(--border)] bg-[var(--surface)] text-gray-300 hover:bg-[var(--border)]/60 hover:text-white"
              }`}
              onClick={() => setShowSetupGuide((v) => !v)}
            >
              {showSetupGuide ? <CloseIcon size={12} /> : <PlusIcon size={12} />}
              <span>{showSetupGuide ? "Închide Ghid" : "Adaugă / Ghid Token"}</span>
            </button>
          )}

          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-white"
              title="Închide fereastra"
            >
              <CloseIcon size={14} />
            </button>
          )}
        </div>
      </div>

      {accounts.length === 0 || showSetupGuide ? (
        <div className="flex-1 overflow-y-auto p-6 flex flex-col items-center justify-center bg-[var(--bg)]/40">
          <div className="w-full max-w-2xl bg-[var(--surface)] border border-[var(--border)] rounded-xl p-6 shadow-xl">
            <div className="flex items-center gap-3 pb-4 border-b border-[var(--border)]">
              <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-orange-500/10 border border-orange-500/20 text-orange-400">
                <CloudIcon size={22} />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-white">Conectare Cont Cloudflare</h3>
                <p className="text-xs text-gray-400 mt-0.5">
                  Generează un API Token cu permisiunile necesare pentru Tunele Zero Trust, DNS și Securitate.
                </p>
              </div>
            </div>

            {/* Steps in clean minimal cards */}
            <div className="mt-4 space-y-3">
              <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3.5 flex items-center justify-between gap-4">
                <div>
                  <span className="text-[10px] font-mono font-semibold uppercase tracking-wider text-orange-400">
                    Pasul 1
                  </span>
                  <div className="text-xs font-medium text-gray-200 mt-0.5">
                    Deschide panoul oficial Cloudflare API Tokens
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => openUrl("https://dash.cloudflare.com/profile/api-tokens")}
                  className="flex items-center gap-1.5 rounded-md border border-orange-500/40 bg-orange-500/10 px-3 py-1.5 text-xs font-medium text-orange-300 hover:bg-orange-500/20 transition"
                >
                  <span>Deschide Cloudflare</span>
                  <GlobeIcon size={12} />
                </button>
              </div>

              <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3.5">
                <span className="text-[10px] font-mono font-semibold uppercase tracking-wider text-orange-400">
                  Pasul 2
                </span>
                <div className="text-xs font-medium text-gray-200 mt-0.5 mb-2">
                  Creează un <strong>Custom Token</strong> cu permisiunile:
                </div>
                <div className="grid grid-cols-2 gap-2 text-[11px] font-mono text-gray-300">
                  <div className="rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1">
                    Account &rarr; Cloudflare Tunnel (Read/Write)
                  </div>
                  <div className="rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1">
                    Zone &rarr; DNS (Read/Write)
                  </div>
                  <div className="rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1">
                    Zone &rarr; Zone Settings (Read/Write)
                  </div>
                  <div className="rounded border border-[var(--border)] bg-[var(--bg)] px-2 py-1">
                    Zone &rarr; Zone (Read)
                  </div>
                </div>
              </div>

              <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3.5">
                <span className="text-[10px] font-mono font-semibold uppercase tracking-wider text-orange-400">
                  Pasul 3
                </span>
                <div className="text-xs font-medium text-gray-200 mt-0.5 mb-2">
                  Lipește token-ul generat pentru autorizare:
                </div>
                <div className="flex gap-2">
                  <TextInput
                    type="password"
                    value={tokenInput}
                    onChange={(e) => setTokenInput(e.target.value)}
                    placeholder="Lipește Cloudflare API Token..."
                    className="flex-1 text-xs font-mono"
                  />
                  <Button
                    variant="primary"
                    disabled={!tokenInput.trim() || savingToken}
                    onClick={handleSaveManualToken}
                    className="text-xs shrink-0"
                  >
                    {savingToken ? "Se conectează..." : "Conectează"}
                  </Button>
                </div>
                {tokenError && (
                  <p className="mt-2 text-xs text-red-400 font-mono">{tokenError}</p>
                )}
              </div>
            </div>
          </div>
        </div>
      ) : (
        <div className="flex flex-1 flex-col min-h-0">
          {/* Refined Navigation Tabs */}
          <div className="flex border-b border-[var(--border)] px-4 bg-[var(--surface)] text-xs">
            <button
              type="button"
              onClick={() => setActiveTab("tunnels")}
              className={`flex items-center gap-1.5 px-3 py-2 border-b-2 font-medium transition ${
                activeTab === "tunnels"
                  ? "border-orange-500 text-orange-400 font-semibold"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              <ShieldIcon size={13} />
              <span>Tunele Zero Trust</span>
              <span className="rounded bg-[var(--border)] px-1.5 py-0.2 text-[10px] text-gray-300 font-mono">
                {tunnels.length}
              </span>
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("dns")}
              className={`flex items-center gap-1.5 px-3 py-2 border-b-2 font-medium transition ${
                activeTab === "dns"
                  ? "border-orange-500 text-orange-400 font-semibold"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              <GlobeIcon size={13} />
              <span>Înregistrări DNS</span>
              <span className="rounded bg-[var(--border)] px-1.5 py-0.2 text-[10px] text-gray-300 font-mono">
                {dnsRecords.length}
              </span>
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("security")}
              className={`flex items-center gap-1.5 px-3 py-2 border-b-2 font-medium transition ${
                activeTab === "security"
                  ? "border-orange-500 text-orange-400 font-semibold"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              <ShieldIcon size={13} />
              <span>Securitate &amp; WAF</span>
            </button>
            <button
              type="button"
              onClick={() => {
                setActiveTab("history");
                if (selectedAccountId) loadHistory(selectedAccountId);
              }}
              className={`flex items-center gap-1.5 px-3 py-2 border-b-2 font-medium transition ${
                activeTab === "history"
                  ? "border-orange-500 text-orange-400 font-semibold"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              <HistoryIcon size={13} />
              <span>Istoric &amp; Rollback</span>
              {history.length > 0 && (
                <span className="rounded bg-orange-500/20 text-orange-300 px-1.5 py-0.2 text-[10px] font-mono font-bold">
                  {history.length}
                </span>
              )}
            </button>
          </div>

          {error && (
            <div className="mx-4 mt-2.5 rounded border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300 font-mono">
              {error}
            </div>
          )}

          {/* TAB 1: TUNNELS */}
          {activeTab === "tunnels" && (
            <div className="flex flex-1 min-h-0 overflow-hidden">
              <div className="w-64 border-r border-[var(--border)] p-3 overflow-y-auto space-y-1.5 bg-[var(--surface-2)]/40">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-[10px] font-semibold uppercase tracking-wider text-gray-400">
                    Tunele Active
                  </span>
                  <button
                    type="button"
                    className="flex items-center gap-1 text-[11px] text-orange-400 hover:text-orange-300"
                    onClick={() => setCreatingTunnel(true)}
                  >
                    <PlusIcon size={11} /> Nou
                  </button>
                </div>

                {tunnels.length === 0 ? (
                  <p className="text-xs text-gray-500 p-2">Nu există tunele create.</p>
                ) : (
                  tunnels.map((t) => {
                    const isSelected = selectedTunnel?.id === t.id;
                    const isHealthy = t.status === "healthy";
                    return (
                      <div
                        key={t.id}
                        onClick={() => selectTunnel(t)}
                        className={`cursor-pointer rounded-md border px-2.5 py-2 text-left transition ${
                          isSelected
                            ? "border-orange-500/60 bg-orange-500/10 text-white"
                            : "border-[var(--border)] bg-[var(--surface)] text-gray-300 hover:bg-[var(--border)]/50"
                        }`}
                      >
                        <div className="flex items-center justify-between">
                          <span className="text-xs font-medium truncate">{t.name}</span>
                          <span
                            className={`h-2 w-2 rounded-full shrink-0 ${
                              isHealthy ? "bg-emerald-400" : "bg-zinc-500"
                            }`}
                            title={`Status: ${t.status || "inactive"}`}
                          />
                        </div>
                        <div className="mt-0.5 text-[10px] text-gray-500 truncate font-mono">
                          {t.id.slice(0, 14)}...
                        </div>
                      </div>
                    );
                  })
                )}
              </div>

              <div className="flex-1 p-5 overflow-y-auto bg-[var(--bg)]/20">
                {selectedTunnel ? (
                  <div className="space-y-4 max-w-3xl">
                    <div className="flex items-center justify-between pb-3 border-b border-[var(--border)]">
                      <div>
                        <div className="flex items-center gap-2">
                          <h3 className="text-sm font-semibold text-white">{selectedTunnel.name}</h3>
                          <span
                            className={`rounded px-1.5 py-0.2 text-[9px] font-mono font-semibold uppercase ${
                              selectedTunnel.status === "healthy"
                                ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/30"
                                : "bg-zinc-800 text-zinc-400 border border-zinc-700"
                            }`}
                          >
                            {selectedTunnel.status || "INACTIVE"}
                          </span>
                        </div>
                        <p className="text-[11px] text-gray-500 font-mono mt-0.5">
                          ID: {selectedTunnel.id}
                        </p>
                      </div>

                      <Button
                        variant="ghost"
                        className="text-xs text-red-400 hover:bg-red-500/10 border border-transparent hover:border-red-500/20"
                        onClick={() => {
                          if (confirm(`Sigur dorești să ștergi tunelul "${selectedTunnel.name}"?`)) {
                            deleteTunnel(selectedTunnel.id);
                          }
                        }}
                      >
                        <TrashIcon size={12} /> Șterge
                      </Button>
                    </div>

                    {tunnelToken && (
                      <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3">
                        <div className="flex items-center justify-between mb-1.5">
                          <span className="text-[11px] font-medium text-gray-300">
                            Comandă instalare &amp; pornire 1-Click pe VPS:
                          </span>
                          <button
                            type="button"
                            onClick={() => {
                              navigator.clipboard.writeText(
                                `curl -L --output cloudflared.deb https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb && sudo dpkg -i cloudflared.deb && sudo cloudflared service install ${tunnelToken}`
                              );
                              setCopiedToken(true);
                              setTimeout(() => setCopiedToken(false), 2000);
                            }}
                            className="flex items-center gap-1 text-[11px] text-orange-400 hover:text-orange-300 font-medium"
                          >
                            {copiedToken ? <CheckIcon size={12} /> : <CopyIcon size={12} />}
                            <span>{copiedToken ? "Copiat" : "Copiază comanda"}</span>
                          </button>
                        </div>
                        <pre className="overflow-x-auto rounded bg-black/50 p-2 text-[11px] font-mono text-gray-300 border border-[var(--border)]">
                          sudo cloudflared service install {tunnelToken}
                        </pre>
                      </div>
                    )}

                    <div>
                      <div className="flex items-center justify-between mb-2">
                        <div>
                          <h4 className="text-xs font-semibold text-gray-200">
                            Rute Ingress (Public Hostname &rarr; Serviciu Intern)
                          </h4>
                        </div>
                        <Button
                          variant="primary"
                          className="text-xs"
                          onClick={() => setAddingIngress(true)}
                        >
                          <PlusIcon size={12} /> Adaugă rută
                        </Button>
                      </div>

                      <div className="overflow-hidden rounded-lg border border-[var(--border)]">
                        <table className="w-full text-left text-xs">
                          <thead className="border-b border-[var(--border)] bg-[var(--surface-2)] text-gray-400">
                            <tr>
                              <th className="p-2 font-medium">Public Hostname</th>
                              <th className="p-2 font-medium">Path</th>
                              <th className="p-2 font-medium">Internal Service</th>
                              <th className="p-2 text-right font-medium w-16">Acțiuni</th>
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-[var(--border)] text-gray-300 font-mono text-[11px]">
                            {tunnelConfig?.ingress && tunnelConfig.ingress.length > 0 ? (
                              tunnelConfig.ingress.map((rule, idx) => (
                                <tr key={idx} className="hover:bg-[var(--surface-hover)]">
                                  <td className="p-2 font-medium text-gray-100 font-sans">
                                    {rule.hostname || (
                                      <span className="text-gray-500 italic">(catch-all)</span>
                                    )}
                                  </td>
                                  <td className="p-2 text-gray-400">{rule.path || "/"}</td>
                                  <td className="p-2 text-cyan-400">{rule.service}</td>
                                  <td className="p-2 text-right">
                                    {rule.hostname && (
                                      <button
                                        type="button"
                                        onClick={() => handleRemoveIngress(idx)}
                                        className="text-gray-400 hover:text-red-400 p-1"
                                        title="Șterge ruta"
                                      >
                                        <TrashIcon size={12} />
                                      </button>
                                    )}
                                  </td>
                                </tr>
                              ))
                            ) : (
                              <tr>
                                <td colSpan={4} className="p-4 text-center text-gray-500 font-sans">
                                  Nicio rută configurată încă.
                                </td>
                              </tr>
                            )}
                          </tbody>
                        </table>
                      </div>
                    </div>
                  </div>
                ) : (
                  <div className="flex h-full items-center justify-center text-xs text-gray-500">
                    Selectează un tunel din lista din stânga.
                  </div>
                )}
              </div>
            </div>
          )}

          {/* TAB 2: DNS RECORDS */}
          {activeTab === "dns" && (
            <div className="flex-1 p-4 overflow-y-auto">
              <div className="flex items-center justify-between mb-3 gap-3">
                <div className="flex items-center gap-2 flex-1">
                  <div className="relative flex items-center flex-1 max-w-xs">
                    <SearchIcon size={12} className="absolute left-2 text-gray-500 pointer-events-none" />
                    <TextInput
                      value={dnsSearch}
                      onChange={(e) => setDnsSearch(e.target.value)}
                      placeholder="Caută înregistrări DNS..."
                      className="pl-6 text-xs"
                    />
                  </div>
                  <Select
                    value={dnsTypeFilter}
                    onChange={(e) => setDnsTypeFilter(e.target.value)}
                    className="text-xs w-28"
                  >
                    <option value="ALL">Toate</option>
                    <option value="A">A</option>
                    <option value="AAAA">AAAA</option>
                    <option value="CNAME">CNAME</option>
                    <option value="TXT">TXT</option>
                    <option value="MX">MX</option>
                  </Select>
                </div>

                <Button
                  variant="primary"
                  className="text-xs"
                  onClick={() =>
                    setEditingDns({
                      name: "",
                      type: "A",
                      content: "",
                      proxied: true,
                      ttl: 1,
                    })
                  }
                >
                  <PlusIcon size={12} /> Adaugă DNS
                </Button>
              </div>

              <div className="overflow-hidden rounded-lg border border-[var(--border)]">
                <table className="w-full text-left text-xs">
                  <thead className="border-b border-[var(--border)] bg-[var(--surface-2)] text-gray-400">
                    <tr>
                      <th className="p-2 font-medium w-16">Tip</th>
                      <th className="p-2 font-medium">Nume</th>
                      <th className="p-2 font-medium">Conținut / Țintă</th>
                      <th className="p-2 font-medium w-24">Proxy</th>
                      <th className="p-2 font-medium w-16">TTL</th>
                      <th className="p-2 text-right font-medium w-20">Acțiuni</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-[var(--border)] text-gray-300">
                    {filteredDns.length > 0 ? (
                      filteredDns.map((rec) => (
                        <tr key={rec.id} className="hover:bg-[var(--surface-hover)]">
                          <td className="p-2">
                            <span className="rounded bg-zinc-800 border border-zinc-700 px-1.5 py-0.2 text-[10px] font-mono font-bold text-gray-200">
                              {rec.type}
                            </span>
                          </td>
                          <td className="p-2 font-medium text-gray-100">{rec.name}</td>
                          <td className="p-2 font-mono text-gray-300 text-[11px]">{rec.content}</td>
                          <td className="p-2">
                            {rec.proxied ? (
                              <span className="inline-flex items-center gap-1 rounded bg-orange-500/10 border border-orange-500/30 px-1.5 py-0.2 text-[10px] font-mono font-semibold text-orange-300">
                                Proxied
                              </span>
                            ) : (
                              <span className="inline-flex items-center gap-1 rounded bg-zinc-800 border border-zinc-700 px-1.5 py-0.2 text-[10px] font-mono text-zinc-400">
                                DNS Only
                              </span>
                            )}
                          </td>
                          <td className="p-2 text-gray-400 text-[11px] font-mono">
                            {rec.ttl === 1 ? "Auto" : `${rec.ttl}s`}
                          </td>
                          <td className="p-2 text-right">
                            <div className="flex justify-end gap-1">
                              <button
                                type="button"
                                className="text-gray-400 hover:text-white px-1.5 py-0.5 text-[11px]"
                                onClick={() =>
                                  setEditingDns({
                                    id: rec.id,
                                    name: rec.name,
                                    type: rec.type,
                                    content: rec.content,
                                    proxied: rec.proxied,
                                    ttl: rec.ttl,
                                  })
                                }
                              >
                                Edit
                              </button>
                              <button
                                type="button"
                                className="text-gray-400 hover:text-red-400 p-1"
                                onClick={() => {
                                  if (confirm(`Ștergi înregistrarea ${rec.type} ${rec.name}?`)) {
                                    deleteDnsRecord(rec.id);
                                  }
                                }}
                              >
                                <TrashIcon size={12} />
                              </button>
                            </div>
                          </td>
                        </tr>
                      ))
                    ) : (
                      <tr>
                        <td colSpan={6} className="p-6 text-center text-gray-500">
                          Nu au fost găsite înregistrări DNS.
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* TAB 3: SECURITY & WAF */}
          {activeTab === "security" && (
            <div className="flex-1 p-5 overflow-y-auto max-w-2xl space-y-4">
              <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <h4 className="text-xs font-semibold text-gray-100 uppercase tracking-wide">
                      "I'm Under Attack" Mode
                    </h4>
                    <p className="text-xs text-gray-400 mt-1 max-w-md">
                      Activează o verificare JavaScript avansată pentru fiecare vizitator pentru a mitiga atacurile DDoS.
                    </p>
                  </div>

                  <Button
                    variant="primary"
                    onClick={() => toggleUnderAttackMode()}
                    className="text-xs"
                  >
                    Comută Mod
                  </Button>
                </div>
              </div>
            </div>
          )}

          {/* TAB 4: HISTORY & ROLLBACK */}
          {activeTab === "history" && (
            <div className="flex-1 overflow-y-auto p-5 space-y-3">
              <div className="flex items-center justify-between pb-2 border-b border-[var(--border)]">
                <h3 className="text-xs font-semibold text-gray-200 uppercase tracking-wider">
                  Audit Log &amp; Rollback
                </h3>

                <button
                  type="button"
                  className="flex items-center gap-1 text-xs text-gray-400 hover:text-white"
                  onClick={() => selectedAccountId && loadHistory(selectedAccountId)}
                >
                  <RefreshIcon size={12} /> Reîmprospătează
                </button>
              </div>

              {revertToast && (
                <div className="rounded border border-emerald-500/30 bg-emerald-950/40 p-2.5 text-xs text-emerald-300 flex items-center justify-between">
                  <span>{revertToast}</span>
                  <button type="button" onClick={() => setRevertToast(null)} className="text-emerald-400 hover:text-white">
                    <CloseIcon size={12} />
                  </button>
                </div>
              )}

              {history.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[var(--border)] p-8 text-center text-xs text-gray-500">
                  Nicio modificare înregistrată în istoric.
                </div>
              ) : (
                <div className="space-y-2">
                  {history.map((log) => (
                    <div key={log.id} className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3">
                      <div className="flex items-start justify-between gap-4">
                        <div>
                          <div className="text-xs font-medium text-gray-200">{log.summary}</div>
                          <div className="text-[10px] text-gray-500 font-mono mt-0.5">{log.created_at}</div>
                        </div>
                        <Button
                          variant="ghost"
                          className="bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/30 text-xs py-1"
                          disabled={revertingId === log.id}
                          onClick={() => handleRevert(log.id)}
                        >
                          {revertingId === log.id ? "Se anulează..." : "Revert"}
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* CREATE TUNNEL MODAL */}
      {creatingTunnel && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-6"
          onMouseDown={(e) => e.target === e.currentTarget && setCreatingTunnel(false)}
        >
          <div className="w-[min(440px,90vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
            <h3 className="text-sm font-semibold text-gray-100 mb-3">Creare tunel Zero Trust nou</h3>
            <Field label="Nume tunel" hint="Exemplu: production-vps, api-gateway">
              <TextInput
                value={newTunnelName}
                onChange={(e) => setNewTunnelName(e.target.value)}
                placeholder="nume-tunel"
                autoFocus
              />
            </Field>
            <div className="flex justify-end gap-2 mt-4">
              <Button variant="ghost" onClick={() => setCreatingTunnel(false)}>
                Anulează
              </Button>
              <Button
                variant="primary"
                onClick={handleCreateTunnel}
                disabled={!newTunnelName.trim()}
              >
                Creează tunel
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* ADD INGRESS RULE MODAL */}
      {addingIngress && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-6"
          onMouseDown={(e) => e.target === e.currentTarget && setAddingIngress(false)}
        >
          <div className="w-[min(480px,90vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl space-y-3">
            <h3 className="text-sm font-semibold text-gray-100">Adăugare rută Ingress</h3>
            <Field label="Domeniu public (Hostname)" hint="Exemplu: app.domeniul-tau.com">
              <TextInput
                value={ingressForm.hostname ?? ""}
                onChange={(e) => setIngressForm((f) => ({ ...f, hostname: e.target.value }))}
                placeholder="app.exemplu.com"
                autoFocus
              />
            </Field>
            <Field label="Serviciu intern (Local / VPS)" hint="Exemplu: http://localhost:3000">
              <TextInput
                value={ingressForm.service}
                onChange={(e) => setIngressForm((f) => ({ ...f, service: e.target.value }))}
                placeholder="http://localhost:3000"
              />
            </Field>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="ghost" onClick={() => setAddingIngress(false)}>
                Anulează
              </Button>
              <Button
                variant="primary"
                onClick={handleAddIngress}
                disabled={!ingressForm.service.trim()}
              >
                Salvează ruta
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* EDIT / ADD DNS MODAL */}
      {editingDns && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-6"
          onMouseDown={(e) => e.target === e.currentTarget && setEditingDns(null)}
        >
          <div className="w-[min(480px,90vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl space-y-3">
            <h3 className="text-sm font-semibold text-gray-100">
              {editingDns.id ? "Editare înregistrare DNS" : "Adăugare înregistrare DNS"}
            </h3>

            <div className="grid grid-cols-3 gap-2">
              <Field label="Tip">
                <Select
                  value={editingDns.type}
                  onChange={(e) => setEditingDns({ ...editingDns, type: e.target.value })}
                >
                  <option value="A">A</option>
                  <option value="AAAA">AAAA</option>
                  <option value="CNAME">CNAME</option>
                  <option value="TXT">TXT</option>
                  <option value="MX">MX</option>
                </Select>
              </Field>

              <div className="col-span-2">
                <Field label="Nume">
                  <TextInput
                    value={editingDns.name}
                    onChange={(e) => setEditingDns({ ...editingDns, name: e.target.value })}
                    placeholder="@ sau api"
                    autoFocus
                  />
                </Field>
              </div>
            </div>

            <Field label="Conținut / IP">
              <TextInput
                value={editingDns.content}
                onChange={(e) => setEditingDns({ ...editingDns, content: e.target.value })}
                placeholder="192.0.2.1"
              />
            </Field>

            <div className="flex justify-end gap-2 pt-3">
              <Button variant="ghost" onClick={() => setEditingDns(null)}>
                Anulează
              </Button>
              <Button
                variant="primary"
                onClick={handleSaveDns}
                disabled={!editingDns.name.trim() || !editingDns.content.trim()}
              >
                Salvează DNS
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
