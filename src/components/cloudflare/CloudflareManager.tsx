import { useEffect, useState } from "react";
import { useCloudflareStore } from "../../stores/cloudflareStore";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type CloudflareDnsRecordInput, type CloudflareIngressRule } from "../../lib/tauri";
import { PlusIcon, TrashIcon } from "../icons";
import { Button, Card, Field, Select, TextInput } from "../settings/ui";

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
    securitySettings,
    error,
    loadAccounts,
    selectAccount,
    selectZone,
    selectTunnel,
    createTunnel,
    deleteTunnel,
    saveTunnelConfig,
    upsertDnsRecord,
    deleteDnsRecord,
    setSecurityLevel,
    toggleUnderAttackMode,
  } = useCloudflareStore();

  const [activeTab, setActiveTab] = useState<"tunnels" | "dns" | "security">("tunnels");
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

  const [loggingIn, setLoggingIn] = useState(false);
  const [copiedToken, setCopiedToken] = useState(false);

  useEffect(() => {
    loadAccounts();
  }, [loadAccounts]);

  const handle1ClickLogin = async () => {
    setLoggingIn(true);
    try {
      const authUrl = await api.startCloudflareOAuthLogin();
      await openUrl(authUrl);
      // Poll accounts
      let count = 0;
      const interval = setInterval(async () => {
        count++;
        await loadAccounts();
        if (count > 30) {
          clearInterval(interval);
          setLoggingIn(false);
        }
      }, 2000);
    } catch (e) {
      alert(`Eroare la conectare: ${e}`);
      setLoggingIn(false);
    }
  };

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
          hostname: ingressForm.hostname?.trim() || null,
          path: ingressForm.path?.trim() || null,
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

  return (
    <div className="flex h-full flex-col bg-[var(--surface)] text-[var(--text)]">
      {/* Header bar */}
      <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-3.5 bg-[var(--surface-2)]">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="text-xl">☁️</span>
            <h2 className="text-sm font-semibold text-gray-100">Cloudflare Zero Trust &amp; Security</h2>
          </div>

          {accounts.length > 0 ? (
            <div className="flex items-center gap-2">
              <Select
                value={selectedAccountId ?? ""}
                onChange={(e) => selectAccount(e.target.value)}
                className="text-xs py-1"
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
                  className="text-xs py-1"
                >
                  {zones.map((z) => (
                    <option key={z.id} value={z.id}>
                      🌐 {z.name}
                    </option>
                  ))}
                </Select>
              )}
            </div>
          ) : null}
        </div>

        <div className="flex items-center gap-2">
          {accounts.length === 0 ? (
            <Button
              variant="primary"
              className="bg-[#f48120] hover:bg-[#e06d0e] text-white text-xs border-none"
              disabled={loggingIn}
              onClick={handle1ClickLogin}
            >
              {loggingIn ? "Așteptare conectare…" : "☁️ 1-Click Login cu Cloudflare"}
            </Button>
          ) : (
            <Button
              variant="ghost"
              className="text-xs text-gray-400 hover:text-white"
              onClick={handle1ClickLogin}
              title="Adaugă un alt cont Cloudflare sau reconectează-te"
            >
              + Conectează alt cont
            </Button>
          )}

          {onClose && (
            <Button variant="ghost" onClick={onClose} className="text-xs">
              Închide
            </Button>
          )}
        </div>
      </div>

      {accounts.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
          <div className="text-4xl mb-3">☁️</div>
          <h3 className="text-base font-semibold text-gray-100 mb-1">Niciun cont Cloudflare conectat</h3>
          <p className="text-xs text-gray-400 max-w-md mb-6">
            Conectează-te în 1-Click pentru a accesa și gestiona tunelele Zero Trust, rutele de ingress către servere, înregistrările DNS și nivelul de securitate WAF.
          </p>
          <Button
            variant="primary"
            className="bg-[#f48120] hover:bg-[#e06d0e] text-white px-6 py-2 border-none"
            disabled={loggingIn}
            onClick={handle1ClickLogin}
          >
            {loggingIn ? "Așteptare autorizare în browser…" : "☁️ Conectare 1-Click cu Cloudflare"}
          </Button>
        </div>
      ) : (
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Navigation Tabs */}
          <div className="flex border-b border-[var(--border)] px-5 bg-[var(--surface)]">
            <button
              type="button"
              onClick={() => setActiveTab("tunnels")}
              className={`px-4 py-2.5 text-xs font-medium border-b-2 transition ${
                activeTab === "tunnels"
                  ? "border-[#f48120] text-[#f48120]"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              🛡️ Tunele Zero Trust ({tunnels.length})
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("dns")}
              className={`px-4 py-2.5 text-xs font-medium border-b-2 transition ${
                activeTab === "dns"
                  ? "border-[#f48120] text-[#f48120]"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              🌐 Înregistrări DNS ({dnsRecords.length})
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("security")}
              className={`px-4 py-2.5 text-xs font-medium border-b-2 transition ${
                activeTab === "security"
                  ? "border-[#f48120] text-[#f48120]"
                  : "border-transparent text-gray-400 hover:text-gray-200"
              }`}
            >
              🔒 Securitate &amp; WAF {securitySettings?.attack_mode && "🚨 Under Attack"}
            </button>
          </div>

          {error && (
            <div className="mx-5 mt-3 rounded-md border border-red-500/30 bg-red-500/10 p-3 text-xs text-red-300">
              {error}
            </div>
          )}

          {/* TAB 1: TUNNELS */}
          {activeTab === "tunnels" && (
            <div className="flex flex-1 min-h-0 overflow-hidden">
              {/* Left sidebar: Tunnel list */}
              <div className="w-72 border-r border-[var(--border)] p-3 overflow-y-auto space-y-2">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-xs font-semibold uppercase tracking-wider text-gray-400">
                    Tunele ({tunnels.length})
                  </span>
                  <Button
                    variant="ghost"
                    className="text-xs p-1 text-[#f48120]"
                    onClick={() => setCreatingTunnel(true)}
                  >
                    <PlusIcon size={12} /> Tunel nou
                  </Button>
                </div>

                {tunnels.length === 0 ? (
                  <p className="text-xs text-gray-500 p-2">Nu există tunele create în acest cont.</p>
                ) : (
                  tunnels.map((t) => {
                    const isSelected = selectedTunnel?.id === t.id;
                    const isHealthy = t.status === "healthy";
                    const isDown = t.status === "down";
                    return (
                      <div
                        key={t.id}
                        onClick={() => selectTunnel(t)}
                        className={`cursor-pointer rounded-lg border p-2.5 text-left transition ${
                          isSelected
                            ? "border-[#f48120] bg-[var(--surface-hover)]"
                            : "border-[var(--border)] hover:bg-[var(--surface-hover)]"
                        }`}
                      >
                        <div className="flex items-center justify-between">
                          <span className="text-xs font-medium text-gray-100 truncate">{t.name}</span>
                          <span
                            className={`inline-block h-2 w-2 rounded-full ${
                              isHealthy ? "bg-green-500" : isDown ? "bg-red-500" : "bg-gray-400"
                            }`}
                            title={`Status: ${t.status || "unknown"}`}
                          />
                        </div>
                        <div className="mt-1 text-[10px] text-gray-500 truncate font-mono">
                          ID: {t.id.slice(0, 12)}…
                        </div>
                      </div>
                    );
                  })
                )}
              </div>

              {/* Right content: Selected Tunnel Details & Ingress Rules */}
              <div className="flex-1 p-5 overflow-y-auto">
                {selectedTunnel ? (
                  <div className="space-y-5 max-w-3xl">
                    <div className="flex items-start justify-between">
                      <div>
                        <h3 className="text-base font-semibold text-gray-100 flex items-center gap-2">
                          {selectedTunnel.name}
                          <span
                            className={`rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase ${
                              selectedTunnel.status === "healthy"
                                ? "bg-green-500/20 text-green-400"
                                : "bg-red-500/20 text-red-400"
                            }`}
                          >
                            {selectedTunnel.status || "INACTIVE"}
                          </span>
                        </h3>
                        <p className="text-xs text-gray-500 font-mono mt-0.5">
                          Tunnel ID: {selectedTunnel.id}
                        </p>
                      </div>

                      <Button
                        variant="ghost"
                        className="text-xs text-red-400 hover:bg-red-500/10"
                        onClick={() => {
                          if (confirm(`Sigur dorești să ștergi tunelul "${selectedTunnel.name}"?`)) {
                            deleteTunnel(selectedTunnel.id);
                          }
                        }}
                      >
                        <TrashIcon size={13} /> Șterge tunel
                      </Button>
                    </div>

                    {/* Quick Run / Install on VPS box */}
                    {tunnelToken && (
                      <Card className="p-3 bg-[var(--surface-2)]">
                        <div className="flex items-center justify-between mb-1.5">
                          <span className="text-xs font-semibold text-gray-200">
                            🚀 Comandă instalare &amp; rulare pe VPS:
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
                            className="text-[11px] text-[#f48120] hover:underline"
                          >
                            {copiedToken ? "Copiat! ✓" : "Copiază comanda 1-Click"}
                          </button>
                        </div>
                        <pre className="overflow-x-auto rounded bg-black/40 p-2 text-[11px] font-mono text-gray-300">
                          sudo cloudflared service install {tunnelToken}
                        </pre>
                      </Card>
                    )}

                    {/* Ingress Rules (Routing) */}
                    <div>
                      <div className="flex items-center justify-between mb-2">
                        <div>
                          <h4 className="text-xs font-semibold text-gray-200">
                            Rute Ingress (Public Hostname &rarr; Local/VPS Service)
                          </h4>
                          <p className="text-[11px] text-gray-500">
                            Ce domeniu public direcționează către ce port/serviciu intern.
                          </p>
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
                              <th className="p-2.5 font-medium">Public Hostname</th>
                              <th className="p-2.5 font-medium">Path</th>
                              <th className="p-2.5 font-medium">Internal Service</th>
                              <th className="p-2.5 text-right font-medium">Acțiuni</th>
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-[var(--border)] text-gray-300">
                            {tunnelConfig?.ingress && tunnelConfig.ingress.length > 0 ? (
                              tunnelConfig.ingress.map((rule, idx) => (
                                <tr key={idx} className="hover:bg-[var(--surface-hover)]">
                                  <td className="p-2.5 font-medium text-gray-100">
                                    {rule.hostname || (
                                      <span className="text-gray-500 italic">(catch-all)</span>
                                    )}
                                  </td>
                                  <td className="p-2.5 text-gray-400">{rule.path || "/"}</td>
                                  <td className="p-2.5 font-mono text-blue-400">{rule.service}</td>
                                  <td className="p-2.5 text-right">
                                    {rule.hostname && (
                                      <button
                                        type="button"
                                        onClick={() => handleRemoveIngress(idx)}
                                        className="text-red-400 hover:text-red-300 p-1"
                                        title="Șterge ruta"
                                      >
                                        <TrashIcon size={13} />
                                      </button>
                                    )}
                                  </td>
                                </tr>
                              ))
                            ) : (
                              <tr>
                                <td colSpan={4} className="p-4 text-center text-gray-500">
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
                    Selectează un tunel din stânga pentru a vedea detaliile și rutele de ingress.
                  </div>
                )}
              </div>
            </div>
          )}

          {/* TAB 2: DNS RECORDS */}
          {activeTab === "dns" && (
            <div className="flex-1 p-5 overflow-y-auto">
              <div className="flex items-center justify-between mb-4 gap-3">
                <div className="flex items-center gap-2 flex-1">
                  <TextInput
                    value={dnsSearch}
                    onChange={(e) => setDnsSearch(e.target.value)}
                    placeholder="Caută înregistrări DNS (nume, IP, valoare)…"
                    className="max-w-xs text-xs"
                  />
                  <Select
                    value={dnsTypeFilter}
                    onChange={(e) => setDnsTypeFilter(e.target.value)}
                    className="text-xs w-28"
                  >
                    <option value="ALL">Toate tipurile</option>
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
                      comment: "",
                    })
                  }
                >
                  <PlusIcon size={12} /> Adaugă înregistrare DNS
                </Button>
              </div>

              <div className="overflow-hidden rounded-lg border border-[var(--border)]">
                <table className="w-full text-left text-xs">
                  <thead className="border-b border-[var(--border)] bg-[var(--surface-2)] text-gray-400">
                    <tr>
                      <th className="p-2.5 font-medium w-16">Tip</th>
                      <th className="p-2.5 font-medium">Nume</th>
                      <th className="p-2.5 font-medium">Conținut / Țintă</th>
                      <th className="p-2.5 font-medium w-24">Proxy Status</th>
                      <th className="p-2.5 font-medium w-16">TTL</th>
                      <th className="p-2.5 text-right font-medium w-24">Acțiuni</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-[var(--border)] text-gray-300">
                    {filteredDns.length > 0 ? (
                      filteredDns.map((rec) => (
                        <tr key={rec.id} className="hover:bg-[var(--surface-hover)]">
                          <td className="p-2.5 font-bold text-gray-200">{rec.type}</td>
                          <td className="p-2.5 font-medium text-gray-100">{rec.name}</td>
                          <td className="p-2.5 font-mono text-gray-300">{rec.content}</td>
                          <td className="p-2.5">
                            {rec.proxied ? (
                              <span className="inline-flex items-center gap-1 rounded bg-orange-500/20 px-1.5 py-0.5 text-[10px] font-semibold text-orange-400">
                                ☁️ Proxied
                              </span>
                            ) : (
                              <span className="inline-flex items-center gap-1 rounded bg-gray-500/20 px-1.5 py-0.5 text-[10px] font-semibold text-gray-400">
                                DNS only
                              </span>
                            )}
                          </td>
                          <td className="p-2.5 text-gray-400">{rec.ttl === 1 ? "Auto" : `${rec.ttl}s`}</td>
                          <td className="p-2.5 text-right">
                            <div className="flex justify-end gap-1">
                              <Button
                                variant="ghost"
                                className="text-xs p-1"
                                onClick={() =>
                                  setEditingDns({
                                    id: rec.id,
                                    name: rec.name,
                                    type: rec.type,
                                    content: rec.content,
                                    proxied: rec.proxied,
                                    ttl: rec.ttl,
                                    comment: rec.comment ?? "",
                                  })
                                }
                              >
                                Edit
                              </Button>
                              <Button
                                variant="ghost"
                                className="text-xs text-red-400 p-1"
                                onClick={() => {
                                  if (confirm(`Ștergi înregistrarea ${rec.type} ${rec.name}?`)) {
                                    deleteDnsRecord(rec.id);
                                  }
                                }}
                              >
                                <TrashIcon size={12} />
                              </Button>
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
            <div className="flex-1 p-5 overflow-y-auto max-w-3xl space-y-5">
              {/* Under Attack Mode Card */}
              <Card
                className={`p-4 border transition ${
                  securitySettings?.attack_mode
                    ? "border-red-500 bg-red-500/10 shadow-lg shadow-red-500/10"
                    : "border-[var(--border)]"
                }`}
              >
                <div className="flex items-center justify-between">
                  <div>
                    <h4 className="text-sm font-semibold text-gray-100 flex items-center gap-2">
                      🚨 "I'm Under Attack" Mode
                      {securitySettings?.attack_mode && (
                        <span className="animate-pulse rounded bg-red-500 px-2 py-0.5 text-[10px] font-bold text-white uppercase">
                          ACTIVAT
                        </span>
                      )}
                    </h4>
                    <p className="text-xs text-gray-400 mt-1 max-w-lg">
                      Activează o verificare JavaScript avansată pentru fiecare vizitator pentru a mitiga atacurile DDoS layer 7.
                    </p>
                  </div>

                  <Button
                    variant="primary"
                    className={
                      securitySettings?.attack_mode
                        ? "bg-red-600 hover:bg-red-700 text-white"
                        : "bg-gray-700 hover:bg-gray-600 text-gray-200"
                    }
                    onClick={() => toggleUnderAttackMode()}
                  >
                    {securitySettings?.attack_mode ? "Oprește Under Attack" : "Activează Under Attack"}
                  </Button>
                </div>
              </Card>

              {/* Security Level Selector */}
              <Card className="p-4">
                <h4 className="text-sm font-semibold text-gray-100 mb-1">Nivel de Securitate WAF</h4>
                <p className="text-xs text-gray-400 mb-4">
                  Alege cât de strict evaluează Cloudflare vizitatorii pe baza reputației IP-ului.
                </p>

                <div className="grid grid-cols-2 md:grid-cols-4 gap-2">
                  {[
                    { id: "essentially_off", label: "Essentially Off", desc: "Verifică doar cele mai grave amenințări" },
                    { id: "low", label: "Low", desc: "Recomandat pentru site-uri cu mulți utilizatori legitimi" },
                    { id: "medium", label: "Medium", desc: "Echilibru optim între securitate și confort (implicit)" },
                    { id: "high", label: "High", desc: "Provoacă toți vizitatorii cu comportament suspect în ultimele 14 zile" },
                  ].map((lvl) => {
                    const isSelected = securitySettings?.security_level === lvl.id;
                    return (
                      <div
                        key={lvl.id}
                        onClick={() => setSecurityLevel(lvl.id)}
                        className={`cursor-pointer rounded-lg border p-3 transition ${
                          isSelected
                            ? "border-[#f48120] bg-[var(--surface-hover)] text-[#f48120]"
                            : "border-[var(--border)] hover:bg-[var(--surface-hover)] text-gray-300"
                        }`}
                      >
                        <div className="font-semibold text-xs mb-1">{lvl.label}</div>
                        <div className="text-[11px] text-gray-400 leading-tight">{lvl.desc}</div>
                      </div>
                    );
                  })}
                </div>
              </Card>

              {/* SSL/TLS Info */}
              <Card className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <h4 className="text-sm font-semibold text-gray-100">Criptare SSL/TLS</h4>
                    <p className="text-xs text-gray-400 mt-0.5">
                      Statusul conexiunii criptate între vizitatori, Cloudflare Edge și serverul tău.
                    </p>
                  </div>
                  <span className="rounded bg-blue-500/20 px-2.5 py-1 text-xs font-semibold text-blue-400 uppercase">
                    {securitySettings?.ssl || "FULL (STRICT)"}
                  </span>
                </div>
              </Card>
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
          <div className="w-[min(460px,90vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
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
                Creează tunel &rarr;
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
            <Field label="Serviciu intern (Local / VPS)" hint="Exemplu: http://localhost:3000 sau tcp://localhost:22">
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
                Salvează ruta &check;
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
          <div className="w-[min(500px,90vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl space-y-3">
            <h3 className="text-sm font-semibold text-gray-100">
              {editingDns.id ? "Editare înregistrare DNS" : "Adăugare înregistrare DNS nouă"}
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
                <Field label="Nume (subdomeniu sau @)">
                  <TextInput
                    value={editingDns.name}
                    onChange={(e) => setEditingDns({ ...editingDns, name: e.target.value })}
                    placeholder="@ sau api"
                    autoFocus
                  />
                </Field>
              </div>
            </div>

            <Field label="Conținut / IP / Țintă">
              <TextInput
                value={editingDns.content}
                onChange={(e) => setEditingDns({ ...editingDns, content: e.target.value })}
                placeholder="192.0.2.1 sau cname.exemplu.com"
              />
            </Field>

            <label className="flex items-center gap-2 text-xs text-gray-300 cursor-pointer pt-1">
              <input
                type="checkbox"
                checked={editingDns.proxied}
                onChange={(e) => setEditingDns({ ...editingDns, proxied: e.target.checked })}
                className="rounded"
              />
              <span>Proxy Cloudflare activat (Orange Cloud ☁️ - Protecție DDoS, WAF, SSL)</span>
            </label>

            <div className="flex justify-end gap-2 pt-3">
              <Button variant="ghost" onClick={() => setEditingDns(null)}>
                Anulează
              </Button>
              <Button
                variant="primary"
                onClick={handleSaveDns}
                disabled={!editingDns.name.trim() || !editingDns.content.trim()}
              >
                Salvează înregistrarea &check;
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
