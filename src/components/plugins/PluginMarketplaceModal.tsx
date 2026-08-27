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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4 md:p-6"
      onMouseDown={(e) => e.target === e.currentTarget && closeMarketplace()}
    >
      <div className="flex h-[90vh] w-[min(1000px,96vw)] flex-col rounded-2xl border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] shadow-2xl overflow-hidden">
        {/* If a plugin is selected for its dedicated page */}
        {selectedPluginId ? (
          <PluginDetailView
            pluginId={selectedPluginId}
            onBack={() => selectPlugin(null)}
          />
        ) : (
          <>
            {/* Header */}
            <div className="flex items-center justify-between border-b border-[var(--border)] px-6 py-4 bg-[var(--surface-2)]">
              <div className="flex items-center gap-3">
                <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-500/20 to-purple-600/30 border border-indigo-500/30 text-2xl">
                  🧩
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    <h2 className="text-base font-semibold text-gray-100">
                      xConsole Plugin Harness
                    </h2>
                    <span className="rounded-full bg-indigo-500/20 text-indigo-300 border border-indigo-500/40 px-2 py-0.5 text-[10px] font-mono">
                      Microkernel Architecture
                    </span>
                  </div>
                  <p className="text-xs text-gray-400">
                    Arhitectură microkernel modulară &bull; Instalează, configurează și citește documentația completă
                  </p>
                </div>
              </div>

              <Button variant="ghost" onClick={closeMarketplace} className="text-xs">
                Închide ✕
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
                    placeholder="Introdu URL GitHub (e.g. DemOnJR/xconsole-plugin-redis sau calea locală) …"
                    className="text-xs font-mono w-full"
                  />
                </div>
                <Button
                  variant="primary"
                  disabled={installing || !sourceInput.trim()}
                  onClick={() => handleInstall(sourceInput)}
                  className="bg-indigo-600 hover:bg-indigo-500 text-white text-xs whitespace-nowrap px-4 py-2 border-none font-medium flex items-center justify-center gap-1.5"
                >
                  {installing ? (
                    <>
                      <span className="animate-spin text-sm">⠋</span> Se instalează…
                    </>
                  ) : (
                    <>
                      <span>🚀</span> 1-Click Install
                    </>
                  )}
                </Button>
              </div>

              {installError && (
                <div className="mt-2.5 flex items-center justify-between rounded-lg bg-red-950/50 border border-red-800/40 px-3 py-2 text-xs text-red-300 animate-in fade-in">
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
                <div className="mt-2.5 flex items-center justify-between rounded-lg bg-emerald-950/50 border border-emerald-800/40 px-3 py-2 text-xs text-emerald-300 animate-in fade-in">
                  <span>✓ {successMsg}</span>
                  <button
                    onClick={() => setSuccessMsg(null)}
                    className="text-emerald-400 hover:text-white text-xs ml-2"
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
                  className={`px-4 py-3 text-xs font-medium border-b-2 transition ${
                    activeTab === "installed"
                      ? "border-indigo-500 text-indigo-400"
                      : "border-transparent text-gray-400 hover:text-gray-200"
                  }`}
                >
                  📦 Pluginuri Instalate ({plugins.length})
                </button>
                <button
                  type="button"
                  onClick={() => setActiveTab("catalog")}
                  className={`px-4 py-3 text-xs font-medium border-b-2 transition ${
                    activeTab === "catalog"
                      ? "border-indigo-500 text-indigo-400"
                      : "border-transparent text-gray-400 hover:text-gray-200"
                  }`}
                >
                  🌐 Catalog Comunitar &amp; Store ({FEATURED_COMMUNITY_PLUGINS.length})
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
                        className={`rounded-full px-2.5 py-0.5 text-[10px] font-mono transition ${
                          categoryFilter === cat
                            ? "bg-indigo-600 text-white font-semibold"
                            : "bg-white/5 text-gray-400 hover:text-gray-200"
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
                    placeholder="Caută pluginuri..."
                    className="rounded-lg bg-[var(--surface)] border border-[var(--border)] px-3 py-1 text-xs text-gray-200 placeholder-gray-500 focus:outline-none focus:border-indigo-500"
                  />
                </div>
              )}
            </div>

            {/* Content Area */}
            <div className="flex-1 overflow-y-auto p-6 space-y-4">
              {activeTab === "installed" ? (
                plugins.length === 0 ? (
                  <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-[var(--border)] p-12 text-center">
                    <div className="text-4xl mb-3">🧩</div>
                    <h3 className="text-sm font-semibold text-gray-200 mb-1">Niciun plugin instalat încă</h3>
                    <p className="text-xs text-gray-400 max-w-md mb-4">
                      Explorează catalogul comunitar sau lipește un repository GitHub pentru a instala instant primul tău plugin modular.
                    </p>
                    <Button variant="primary" onClick={() => setActiveTab("catalog")} className="text-xs">
                      Deschide Catalogul Comunitar &rarr;
                    </Button>
                  </div>
                ) : (
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    {plugins.map((plugin) => {
                      const isEnabled = plugin.enabled !== false;
                      const def = definitions[plugin.id];
                      const hasView = Boolean(def?.renderView);

                      return (
                        <Card
                          key={plugin.id}
                          className={`p-4 flex flex-col justify-between transition border cursor-pointer ${
                            isEnabled
                              ? "border-[var(--border)] hover:border-indigo-500/60 bg-[var(--surface-2)] shadow-sm"
                              : "border-white/5 bg-white/[0.02] opacity-65"
                          }`}
                          onClick={() => selectPlugin(plugin.id)}
                        >
                          <div className="space-y-2.5">
                            <div className="flex items-start justify-between gap-3">
                              <div className="flex items-center gap-2.5">
                                <span className="text-2xl">{plugin.icon || "🧩"}</span>
                                <div>
                                  <div className="flex items-center gap-1.5">
                                    <h4 className="text-xs font-semibold text-gray-100 hover:text-indigo-300 transition">
                                      {plugin.name}
                                    </h4>
                                    <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-gray-400">
                                      v{plugin.version}
                                    </span>
                                  </div>
                                  <div className="text-[10px] text-gray-400 font-mono">
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
                                  <div className="w-9 h-5 bg-gray-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
                                </label>
                              </div>
                            </div>

                            <p className="text-xs text-gray-300 line-clamp-2 leading-relaxed">
                              {plugin.description}
                            </p>

                            {/* Capability Badges */}
                            <div className="flex flex-wrap gap-1 pt-1">
                              {plugin.capabilities?.navItem && (
                                <span className="rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 px-1.5 py-0.5 text-[10px]">
                                  🛡️ Nav: {(plugin.capabilities.navItem as any).label}
                                </span>
                              )}
                              {plugin.capabilities?.agentTools && (plugin.capabilities.agentTools as any[]).length > 0 && (
                                <span className="rounded bg-purple-500/10 text-purple-400 border border-purple-500/20 px-1.5 py-0.5 text-[10px]">
                                  🤖 {(plugin.capabilities.agentTools as any[]).length} Agent Tools
                                </span>
                              )}
                              {plugin.capabilities?.canvasNode && (
                                <span className="rounded bg-blue-500/10 text-blue-400 border border-blue-500/20 px-1.5 py-0.5 text-[10px]">
                                  🎨 Canvas Node
                                </span>
                              )}
                              {plugin.capabilities?.settingsSection && (
                                <span className="rounded bg-amber-500/10 text-amber-400 border border-amber-500/20 px-1.5 py-0.5 text-[10px]">
                                  ⚙️ Settings
                                </span>
                              )}
                            </div>
                          </div>

                          {/* Action buttons */}
                          <div
                            className="flex items-center justify-between pt-3 mt-3 border-t border-white/5"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <button
                              type="button"
                              onClick={() => selectPlugin(plugin.id)}
                              className="text-[11px] text-indigo-400 hover:text-indigo-300 font-medium flex items-center gap-1"
                            >
                              <span>📖 Detalii &amp; README</span>
                              <span>&rarr;</span>
                            </button>

                            <div className="flex items-center gap-1.5">
                              {hasView && isEnabled && (
                                <Button
                                  variant="ghost"
                                  className="text-xs text-indigo-400 hover:text-indigo-300 px-2 py-1"
                                  onClick={() => {
                                    closeMarketplace();
                                    openPluginView(plugin.id);
                                  }}
                                >
                                  Deschide View &rarr;
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
                                    Confirmă
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
                                  className="text-xs text-gray-500 hover:text-red-400 px-1.5 py-1"
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
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {filteredCatalog.map((item) => {
                    const isInstalled = installedIds.has(item.id);

                    return (
                      <Card
                        key={item.id}
                        className="p-4 flex flex-col justify-between border border-[var(--border)] bg-[var(--surface-2)] hover:border-indigo-500/50 transition"
                      >
                        <div className="space-y-2">
                          <div className="flex items-start justify-between gap-3">
                            <div className="flex items-center gap-2.5">
                              <span className="text-2xl">{item.icon}</span>
                              <div>
                                <h4 className="text-xs font-semibold text-gray-100">
                                  {item.name}
                                </h4>
                                <div className="text-[10px] text-gray-400 font-mono">
                                  v{item.version} &bull; {item.author}
                                </div>
                              </div>
                            </div>

                            <span className="rounded bg-indigo-500/10 text-indigo-400 px-2 py-0.5 text-[10px] uppercase font-mono">
                              {item.category}
                            </span>
                          </div>

                          <p className="text-xs text-gray-300 line-clamp-2">
                            {item.description}
                          </p>

                          <div className="flex flex-wrap gap-1 pt-1">
                            {item.tags.map((tag) => (
                              <span
                                key={tag}
                                className="rounded bg-white/5 px-1.5 py-0.2 text-[9px] text-gray-400 font-mono"
                              >
                                #{tag}
                              </span>
                            ))}
                          </div>
                        </div>

                        <div className="flex items-center justify-between pt-4 mt-3 border-t border-white/5">
                          <span className="text-[10px] text-gray-500 font-mono truncate max-w-[200px]">
                            {item.repository}
                          </span>

                          {isInstalled ? (
                            <div className="flex items-center gap-2">
                              <span className="text-xs text-emerald-400 font-medium">
                                ✓ Instalat
                              </span>
                              <Button
                                variant="ghost"
                                className="text-xs text-indigo-400"
                                onClick={() => selectPlugin(item.id)}
                              >
                                Vezi Detalii &rarr;
                              </Button>
                            </div>
                          ) : (
                            <Button
                              variant="primary"
                              className="text-xs bg-indigo-600 hover:bg-indigo-500 text-white"
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
            <div className="flex items-center justify-between border-t border-[var(--border)] px-6 py-3 bg-[var(--surface-2)] text-[11px] text-gray-400 font-mono">
              <div className="flex items-center gap-2">
                <span>💡 CLI 1-Command:</span>
                <code className="rounded bg-black/40 px-2 py-0.5 text-gray-300 border border-[var(--border)]">
                  xconsole plugin install &lt;repo_url_or_name&gt;
                </code>
              </div>
              <div>xConsole Harness Spec v1.0</div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
