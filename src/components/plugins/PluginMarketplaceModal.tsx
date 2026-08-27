import { useState, useEffect, type ChangeEvent, type KeyboardEvent } from "react";
import {
  usePluginStore,
  FEATURED_COMMUNITY_PLUGINS,
} from "../../stores/pluginStore";
import { Button, TextInput, Card } from "../settings/ui";

export function PluginMarketplaceModal() {
  const {
    plugins,
    marketplaceOpen,
    closeMarketplace,
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4 md:p-6"
      onMouseDown={(e) => e.target === e.currentTarget && closeMarketplace()}
    >
      <div className="flex h-[88vh] w-[min(960px,96vw)] flex-col rounded-2xl border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] shadow-2xl overflow-hidden">
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
                Arhitectură microkernel modulară &bull; Instalează și folosește orice extensie cu 1 singură comandă
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
                placeholder="Introdu URL GitHub (e.g. xconsole-plugins/xconsole-plugin-cloudflare sau calea locală)..."
                className="w-full text-xs font-mono"
              />
            </div>
            <Button
              variant="primary"
              className="bg-indigo-600 hover:bg-indigo-500 text-white text-xs px-5 border-none whitespace-nowrap"
              disabled={installing || !sourceInput.trim()}
              onClick={() => handleInstall(sourceInput)}
            >
              {installing ? "Se instalează…" : "🚀 1-Click Install"}
            </Button>
          </div>

          {installError && (
            <div className="mt-2 text-xs text-red-400 bg-red-950/40 border border-red-800/40 rounded-lg p-2 flex items-center justify-between">
              <span>❌ {installError}</span>
              <button type="button" onClick={() => setInstallError(null)} className="text-red-400 hover:text-white">✕</button>
            </div>
          )}

          {successMsg && (
            <div className="mt-2 text-xs text-emerald-400 bg-emerald-950/40 border border-emerald-800/40 rounded-lg p-2 flex items-center justify-between">
              <span>✓ {successMsg}</span>
              <button type="button" onClick={() => setSuccessMsg(null)} className="text-emerald-400 hover:text-white">✕</button>
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
                  return (
                    <Card
                      key={plugin.id}
                      className={`p-4 flex flex-col justify-between transition border ${
                        isEnabled
                          ? "border-[var(--border)] hover:border-indigo-500/50 bg-[var(--surface-2)]"
                          : "border-white/5 bg-white/[0.02] opacity-60"
                      }`}
                    >
                      <div className="space-y-2.5">
                        <div className="flex items-start justify-between gap-3">
                          <div className="flex items-center gap-2.5">
                            <span className="text-2xl">{plugin.icon || "🧩"}</span>
                            <div>
                              <div className="flex items-center gap-1.5">
                                <h4 className="text-xs font-semibold text-gray-100">
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
                          <label className="relative inline-flex items-center cursor-pointer">
                            <input
                              type="checkbox"
                              checked={isEnabled}
                              onChange={() => togglePlugin(plugin.id, !isEnabled)}
                              className="sr-only peer"
                            />
                            <div className="w-9 h-5 bg-gray-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
                          </label>
                        </div>

                        <p className="text-xs text-gray-300 line-clamp-2 leading-relaxed">
                          {plugin.description}
                        </p>

                        {/* Capability Badges */}
                        <div className="flex flex-wrap gap-1 pt-1">
                          {plugin.capabilities?.navItem && (
                            <span className="rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 px-1.5 py-0.5 text-[10px]">
                              🛡️ Nav: {plugin.capabilities.navItem.label}
                            </span>
                          )}
                          {plugin.capabilities?.agentTools && plugin.capabilities.agentTools.length > 0 && (
                            <span className="rounded bg-purple-500/10 text-purple-400 border border-purple-500/20 px-1.5 py-0.5 text-[10px]">
                              🤖 {plugin.capabilities.agentTools.length} Agent Tools
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
                      <div className="flex items-center justify-between pt-4 mt-3 border-t border-white/5">
                        <div className="text-[10px] text-gray-500 font-mono">
                          {plugin.isBuiltin ? "Built-in core plugin" : "Community installed"}
                        </div>

                        <div className="flex items-center gap-2">
                          {plugin.capabilities?.navItem && isEnabled && (
                            <Button
                              variant="ghost"
                              className="text-xs text-indigo-400 hover:text-indigo-300 px-2.5 py-1"
                              onClick={() => {
                                openPluginView(plugin.id);
                                closeMarketplace();
                              }}
                            >
                              Deschide View &rarr;
                            </Button>
                          )}

                          {!plugin.isBuiltin && (
                            <Button
                              variant="ghost"
                              className="text-xs text-red-400 hover:text-red-300 hover:bg-red-950/30 px-2 py-1"
                              onClick={async () => {
                                if (confirm(`Sigur dorești să dezinstalezi pluginul '${plugin.name}'?`)) {
                                  await uninstallPlugin(plugin.id);
                                }
                              }}
                            >
                              Dezinstalează
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
            /* CATALOG TAB */
            <div className="space-y-4">
              <div className="flex items-center gap-2 overflow-x-auto pb-1 text-xs">
                {["all", "infrastructure", "database", "networking"].map((cat) => (
                  <button
                    key={cat}
                    type="button"
                    onClick={() => setCategoryFilter(cat)}
                    className={`rounded-lg px-3 py-1 capitalize transition ${
                      categoryFilter === cat
                        ? "bg-indigo-600 text-white font-medium"
                        : "bg-[var(--surface-2)] text-gray-400 hover:text-white"
                    }`}
                  >
                    {cat}
                  </button>
                ))}
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {filteredCatalog.map((item) => {
                  const isInstalled = installedIds.has(item.id);

                  return (
                    <Card
                      key={item.id}
                      className="p-4 flex flex-col justify-between border border-[var(--border)] hover:border-indigo-500/40 bg-[var(--surface-2)] transition"
                    >
                      <div className="space-y-2.5">
                        <div className="flex items-start justify-between gap-3">
                          <div className="flex items-center gap-2.5">
                            <span className="text-2xl">{item.icon}</span>
                            <div>
                              <div className="flex items-center gap-1.5">
                                <h4 className="text-xs font-semibold text-gray-100">
                                  {item.name}
                                </h4>
                                <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-gray-400">
                                  v{item.version}
                                </span>
                              </div>
                              <div className="text-[10px] text-gray-400 font-mono">
                                de {item.author} &bull; ★ {item.stars}
                              </div>
                            </div>
                          </div>

                          <span className="rounded-full bg-white/5 border border-white/10 px-2 py-0.5 text-[9px] text-gray-300 uppercase">
                            {item.category}
                          </span>
                        </div>

                        <p className="text-xs text-gray-300 leading-relaxed">
                          {item.description}
                        </p>

                        <div className="flex flex-wrap gap-1 pt-1">
                          {item.tags.map((t) => (
                            <span
                              key={t}
                              className="rounded bg-white/5 px-1.5 py-0.2 text-[9px] font-mono text-gray-400"
                            >
                              #{t}
                            </span>
                          ))}
                        </div>
                      </div>

                      <div className="flex items-center justify-between pt-4 mt-3 border-t border-white/5">
                        <span className="text-[10px] text-gray-500 font-mono truncate max-w-[200px]">
                          {item.id}
                        </span>

                        {isInstalled ? (
                          <span className="rounded bg-emerald-500/20 text-emerald-300 border border-emerald-500/30 px-3 py-1 text-xs font-semibold">
                            ✓ Instalat
                          </span>
                        ) : (
                          <Button
                            variant="primary"
                            className="bg-indigo-600 hover:bg-indigo-500 text-white text-xs px-4 py-1 border-none"
                            disabled={installing}
                            onClick={() => handleInstall(item.repository)}
                          >
                            + Instalează
                          </Button>
                        )}
                      </div>
                    </Card>
                  );
                })}
              </div>
            </div>
          )}
        </div>

        {/* Footer info & CLI tip */}
        <div className="flex items-center justify-between border-t border-[var(--border)] px-6 py-3 bg-[var(--surface-2)] text-[11px] text-gray-400">
          <div className="flex items-center gap-2">
            <span>💡 <strong>CLI 1-Command:</strong></span>
            <code className="rounded bg-black/40 border border-white/10 px-2 py-0.5 font-mono text-[10px] text-indigo-300">
              xconsole plugin install &lt;repo_url_or_name&gt;
            </code>
          </div>
          <div className="flex items-center gap-2 text-gray-400">
            <span className="text-[10px] font-mono text-indigo-300/80">xConsole Harness Spec v1.0</span>
          </div>
        </div>
      </div>
    </div>
  );
}
