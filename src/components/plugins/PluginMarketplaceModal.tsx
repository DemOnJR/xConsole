import { useState, useEffect, type ChangeEvent, type KeyboardEvent } from "react";
import {
  usePluginStore,
  FEATURED_COMMUNITY_PLUGINS,
} from "../../stores/pluginStore";
import { Button, TextInput, Card } from "../settings/ui";
import { PluginDetailView } from "./PluginDetailView";

export function PluginMarketplaceModal() {
  const {
    plugins,
    definitions,
    marketplaceOpen,
    closeMarketplace,
    selectedPluginId,
    selectPlugin,
    installPlugin,
    uninstallPlugin,
    togglePlugin,
    openPluginView,
    loadPlugins,
    installing,
  } = usePluginStore();

  const [activeTab, setActiveTab] = useState<"installed" | "catalog">("installed");
  const [sourceInput, setSourceInput] = useState("");
  const [installError, setInstallError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<string>("all");
  const [uninstallConfirmId, setUninstallConfirmId] = useState<string | null>(null);

  useEffect(() => {
    if (marketplaceOpen) {
      loadPlugins();
    }
  }, [marketplaceOpen, loadPlugins]);

  if (!marketplaceOpen) return null;

  const handleInstall = async (source: string) => {
    if (!source.trim()) return;
    setInstallError(null);
    setSuccessMsg(null);
    try {
      const result = await installPlugin(source.trim());
      setSourceInput("");
      setSuccessMsg(`Pluginul '${result.name}' a fost instalat și activat cu succes!`);
      setTimeout(() => setSuccessMsg(null), 4000);
    } catch (e) {
      setInstallError(String(e));
    }
  };

  const installedIds = new Set(plugins.map((p) => p.id));

  const filteredCatalog = FEATURED_COMMUNITY_PLUGINS.filter((item) => {
    const matchesSearch =
      searchQuery === "" ||
      item.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      item.description.toLowerCase().includes(searchQuery.toLowerCase()) ||
      item.tags.some((t) => t.toLowerCase().includes(searchQuery.toLowerCase()));
    const matchesCat = categoryFilter === "all" || item.category === categoryFilter;
    return matchesSearch && matchesCat;
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4 md:p-6"
      onMouseDown={(e) => e.target === e.currentTarget && closeMarketplace()}
    >
      <div className="flex h-[90vh] w-[min(960px,96vw)] flex-col rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-2xl overflow-hidden font-sans">
        {/* If a plugin is selected for its dedicated page */}
        {selectedPluginId ? (
          <PluginDetailView
            pluginId={selectedPluginId}
            onBack={() => selectPlugin(null)}
          />
        ) : (
          <>
            {/* Header */}
            <div className="flex items-center justify-between border-b border-[var(--border)] px-6 py-3.5 bg-[var(--surface-2)]">
              <div className="flex items-center gap-3">
                <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-[var(--surface-hover)] border border-[var(--border)] text-xl">
                  🧩
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    <h2 className="text-sm font-semibold text-gray-100 tracking-tight">
                      xConsole Plugin Harness
                    </h2>
                    <span className="rounded bg-white/10 text-gray-300 border border-white/10 px-1.5 py-0.2 text-[10px] font-mono">
                      Microkernel v1.0
                    </span>
                  </div>
                  <p className="text-[11px] text-[var(--text-faint)]">
                    Extensii modulare independente &bull; Instalează din GitHub sau folder local
                  </p>
                </div>
              </div>

              <Button variant="ghost" onClick={closeMarketplace} className="text-xs text-gray-400 hover:text-white">
                ✕
              </Button>
            </div>

            {/* Quick Install Bar */}
            <div className="border-b border-[var(--border)] bg-[var(--surface)] px-6 py-3">
              <div className="flex flex-col sm:flex-row gap-2">
                <div className="relative flex-1">
                  <TextInput
                    value={sourceInput}
                    onChange={(e: ChangeEvent<HTMLInputElement>) => setSourceInput(e.target.value)}
                    onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => e.key === "Enter" && handleInstall(sourceInput)}
                    placeholder="e.g. DemOnJR/xconsole-plugin-redis sau calea locală..."
                    className="text-xs font-mono w-full bg-[var(--surface-2)] border-[var(--border)]"
                  />
                </div>
                <Button
                  variant="primary"
                  disabled={installing || !sourceInput.trim()}
                  onClick={() => handleInstall(sourceInput)}
                  className="bg-zinc-100 text-zinc-950 hover:bg-white text-xs whitespace-nowrap px-4 py-2 border-none font-medium flex items-center justify-center gap-1.5"
                >
                  {installing ? (
                    <>
                      <span className="animate-spin text-sm">⠋</span> Instalare…
                    </>
                  ) : (
                    <>
                      <span>+</span> Instalează
                    </>
                  )}
                </Button>
              </div>

              {installError && (
                <div className="mt-2.5 flex items-center justify-between rounded-lg bg-red-950/40 border border-red-900/40 px-3 py-2 text-xs text-red-300">
                  <span>❌ {installError}</span>
                  <button
                    onClick={() => setInstallError(null)}
                    className="text-red-400 hover:text-white text-xs ml-2"
                  >
                    ✕
                  </button>
                </div>
              )}

              {successMsg && (
                <div className="mt-2.5 flex items-center justify-between rounded-lg bg-zinc-900 border border-emerald-900/50 px-3 py-2 text-xs text-emerald-300">
                  <span>✓ {successMsg}</span>
                  <button
                    onClick={() => setSuccessMsg(null)}
                    className="text-gray-400 hover:text-white text-xs ml-2"
                  >
                    ✕
                  </button>
                </div>
              )}
            </div>

            {/* Tabs & Filters Header */}
            <div className="flex items-center justify-between border-b border-[var(--border)] px-6 bg-[var(--surface-2)]">
              <div className="flex">
                <button
                  type="button"
                  onClick={() => setActiveTab("installed")}
                  className={`px-4 py-2.5 text-xs font-medium border-b-2 transition ${
                    activeTab === "installed"
                      ? "border-zinc-200 text-white"
                      : "border-transparent text-zinc-400 hover:text-zinc-200"
                  }`}
                >
                  Instalate ({plugins.length})
                </button>
                <button
                  type="button"
                  onClick={() => setActiveTab("catalog")}
                  className={`px-4 py-2.5 text-xs font-medium border-b-2 transition ${
                    activeTab === "catalog"
                      ? "border-zinc-200 text-white"
                      : "border-transparent text-zinc-400 hover:text-zinc-200"
                  }`}
                >
                  Catalog Comunitar ({FEATURED_COMMUNITY_PLUGINS.length})
                </button>
              </div>

              {activeTab === "catalog" && (
                <div className="flex items-center gap-2 py-2">
                  <div className="hidden sm:flex items-center gap-1">
                    {["all", "infrastructure", "database", "networking", "ai", "monitoring"].map((cat) => (
                      <button
                        key={cat}
                        type="button"
                        onClick={() => setCategoryFilter(cat)}
                        className={`rounded px-2 py-0.5 text-[10px] font-mono transition ${
                          categoryFilter === cat
                            ? "bg-zinc-200 text-zinc-950 font-semibold"
                            : "bg-white/5 text-zinc-400 hover:text-zinc-200"
                        }`}
                      >
                        {cat}
                      </button>
                    ))}
                  </div>

                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder="Caută..."
                    className="rounded bg-[var(--surface)] border border-[var(--border)] px-2.5 py-1 text-xs text-gray-200 placeholder-zinc-500 focus:outline-none focus:border-zinc-500"
                  />
                </div>
              )}
            </div>

            {/* Content Area */}
            <div className="flex-1 overflow-y-auto p-6 space-y-4">
              {activeTab === "installed" ? (
                plugins.length === 0 ? (
                  <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-[var(--border)] p-12 text-center">
                    <div className="text-3xl mb-2 text-zinc-500">🧩</div>
                    <h3 className="text-xs font-semibold text-gray-300 mb-1">Niciun plugin instalat</h3>
                    <p className="text-[11px] text-zinc-500 max-w-sm mb-4">
                      Explorează catalogul comunitar sau introdu un repo GitHub pentru a adăuga funcționalități modulare.
                    </p>
                    <Button variant="ghost" onClick={() => setActiveTab("catalog")} className="text-xs border border-[var(--border)]">
                      Explorează Catalogul &rarr;
                    </Button>
                  </div>
                ) : (
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
                    {plugins.map((plugin) => {
                      const isEnabled = plugin.enabled !== false;
                      const def = definitions[plugin.id];
                      const hasView = Boolean(def?.renderView);

                      return (
                        <Card
                          key={plugin.id}
                          className={`p-4 flex flex-col justify-between transition border cursor-pointer ${
                            isEnabled
                              ? "border-[var(--border)] hover:border-zinc-500 bg-[var(--surface-2)]"
                              : "border-white/5 bg-white/[0.02] opacity-55"
                          }`}
                          onClick={() => selectPlugin(plugin.id)}
                        >
                          <div className="space-y-2.5">
                            <div className="flex items-start justify-between gap-3">
                              <div className="flex items-center gap-2.5">
                                <span className="text-xl">{plugin.icon || "🧩"}</span>
                                <div>
                                  <div className="flex items-center gap-1.5">
                                    <h4 className="text-xs font-semibold text-gray-100 hover:text-white transition">
                                      {plugin.name}
                                    </h4>
                                    <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-zinc-400">
                                      v{plugin.version}
                                    </span>
                                  </div>
                                  <div className="text-[10px] text-zinc-500 font-mono">
                                    {plugin.author} &bull; {plugin.category}
                                  </div>
                                </div>
                              </div>

                              {/* Toggle Switch */}
                              <div
                                onClick={(e) => e.stopPropagation()}
                                className="flex items-center"
                              >
                                <label className="relative inline-flex items-center cursor-pointer">
                                  <input
                                    type="checkbox"
                                    checked={isEnabled}
                                    onChange={(e) => {
                                      e.stopPropagation();
                                      togglePlugin(plugin.id, !isEnabled);
                                    }}
                                    className="sr-only peer"
                                  />
                                  <div className="w-8 h-4.5 bg-zinc-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-zinc-950 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-zinc-200 peer-checked:after:bg-zinc-900"></div>
                                </label>
                              </div>
                            </div>

                            <p className="text-xs text-zinc-400 line-clamp-2 leading-relaxed">
                              {plugin.description}
                            </p>

                            {/* Capability Badges */}
                            <div className="flex flex-wrap gap-1 pt-0.5 font-mono text-[10px]">
                              {plugin.capabilities?.navItem && (
                                <span className="rounded bg-zinc-800 text-zinc-300 border border-zinc-700 px-1.5 py-0.5">
                                  nav: {(plugin.capabilities.navItem as any).label}
                                </span>
                              )}
                              {plugin.capabilities?.agentTools && (plugin.capabilities.agentTools as any[]).length > 0 && (
                                <span className="rounded bg-zinc-800 text-zinc-300 border border-zinc-700 px-1.5 py-0.5">
                                  {(plugin.capabilities.agentTools as any[]).length} tools
                                </span>
                              )}
                              {plugin.capabilities?.canvasNode && (
                                <span className="rounded bg-zinc-800 text-zinc-300 border border-zinc-700 px-1.5 py-0.5">
                                  canvas node
                                </span>
                              )}
                              {plugin.capabilities?.settingsSection && (
                                <span className="rounded bg-zinc-800 text-zinc-300 border border-zinc-700 px-1.5 py-0.5">
                                  settings
                                </span>
                              )}
                            </div>
                          </div>

                          {/* Action buttons */}
                          <div
                            className="flex items-center justify-between pt-3 mt-3 border-t border-[var(--border)] text-xs"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <button
                              type="button"
                              onClick={() => selectPlugin(plugin.id)}
                              className="text-[11px] text-zinc-300 hover:text-white font-mono flex items-center gap-1"
                            >
                              <span>docs &rarr;</span>
                            </button>

                            <div className="flex items-center gap-1.5">
                              {hasView && isEnabled && (
                                <Button
                                  variant="ghost"
                                  className="text-xs text-zinc-200 hover:text-white px-2 py-0.5 border border-[var(--border)]"
                                  onClick={() => {
                                    closeMarketplace();
                                    openPluginView(plugin.id);
                                  }}
                                >
                                  Open View
                                </Button>
                              )}

                              {uninstallConfirmId === plugin.id ? (
                                <div className="flex items-center gap-1">
                                  <Button
                                    variant="danger"
                                    className="text-[10px] py-0.5 px-1.5"
                                    onClick={async () => {
                                      await uninstallPlugin(plugin.id);
                                      setUninstallConfirmId(null);
                                    }}
                                  >
                                    Confirm
                                  </Button>
                                  <Button
                                    variant="ghost"
                                    className="text-[10px] py-0.5 px-1.5"
                                    onClick={() => setUninstallConfirmId(null)}
                                  >
                                    ✕
                                  </Button>
                                </div>
                              ) : (
                                <Button
                                  variant="ghost"
                                  className="text-xs text-zinc-500 hover:text-red-400 px-1.5 py-0.5"
                                  title="Dezinstalează plugin"
                                  onClick={() => setUninstallConfirmId(plugin.id)}
                                >
                                  🗑️
                                </Button>
                              )}
                            </div>
                          </div>
                        </Card>
                      );
                    })}
                  </div>
                )
              ) : (
                /* Catalog Tab */
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
                  {filteredCatalog.map((item) => {
                    const isInstalled = installedIds.has(item.id);

                    return (
                      <Card
                        key={item.id}
                        className="p-4 flex flex-col justify-between border border-[var(--border)] bg-[var(--surface-2)] hover:border-zinc-500 transition"
                      >
                        <div className="space-y-2">
                          <div className="flex items-start justify-between gap-3">
                            <div className="flex items-center gap-2.5">
                              <span className="text-xl">{item.icon}</span>
                              <div>
                                <h4 className="text-xs font-semibold text-gray-100">
                                  {item.name}
                                </h4>
                                <div className="text-[10px] text-zinc-500 font-mono">
                                  v{item.version} &bull; {item.author}
                                </div>
                              </div>
                            </div>

                            <span className="rounded bg-white/5 text-zinc-400 border border-white/10 px-1.5 py-0.2 text-[10px] uppercase font-mono">
                              {item.category}
                            </span>
                          </div>

                          <p className="text-xs text-zinc-400 line-clamp-2">
                            {item.description}
                          </p>

                          <div className="flex flex-wrap gap-1 pt-1 font-mono text-[9px]">
                            {item.tags.map((tag) => (
                              <span
                                key={tag}
                                className="rounded bg-white/5 px-1.5 py-0.2 text-zinc-500"
                              >
                                #{tag}
                              </span>
                            ))}
                          </div>
                        </div>

                        <div className="flex items-center justify-between pt-3 mt-3 border-t border-[var(--border)]">
                          <span className="text-[10px] text-zinc-500 font-mono truncate max-w-[200px]">
                            {item.repository.replace("https://github.com/", "")}
                          </span>

                          {isInstalled ? (
                            <div className="flex items-center gap-2">
                              <span className="text-xs text-emerald-400 font-mono">
                                ✓ Instalat
                              </span>
                              <Button
                                variant="ghost"
                                className="text-xs text-zinc-300 hover:text-white"
                                onClick={() => selectPlugin(item.id)}
                              >
                                Detalii &rarr;
                              </Button>
                            </div>
                          ) : (
                            <Button
                              variant="primary"
                              className="text-xs bg-zinc-100 hover:bg-white text-zinc-950 font-medium px-3 py-1"
                              disabled={installing}
                              onClick={() => handleInstall(item.repository)}
                            >
                              Instalează
                            </Button>
                          )}
                        </div>
                      </Card>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Footer Status */}
            <div className="flex items-center justify-between border-t border-[var(--border)] px-6 py-2.5 bg-[var(--surface-2)] text-[11px] text-zinc-500 font-mono">
              <div className="flex items-center gap-2">
                <span>CLI:</span>
                <code className="rounded bg-black/40 px-2 py-0.5 text-zinc-300 border border-[var(--border)]">
                  xconsole plugin install &lt;repo&gt;
                </code>
              </div>
              <div>microkernel harness</div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
