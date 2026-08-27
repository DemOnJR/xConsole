import { useState, useEffect, useRef, type ChangeEvent, type KeyboardEvent } from "react";
import {
  usePluginStore,
  FEATURED_COMMUNITY_PLUGINS,
} from "../../stores/pluginStore";
import { Button, TextInput, Card } from "../settings/ui";
import { PluginDetailView } from "./PluginDetailView";
import { PluginIcon } from "./PluginIcon";
import {
  PuzzleIcon,
  SpinnerIcon,
  CheckIcon,
  XIcon,
  TrashIcon,
  DownloadIcon,
  TerminalIcon,
  RefreshIcon,
} from "../icons";

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
    installProgress,
    clearInstallProgress,
    availableUpdates,
    checkingUpdates,
    updatingPluginIds,
    checkForUpdates,
    updateSinglePlugin,
    updateAllAvailablePlugins,
  } = usePluginStore();

  const [activeTab, setActiveTab] = useState<"installed" | "catalog">("installed");
  const [sourceInput, setSourceInput] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<string>("all");
  const [uninstallConfirmId, setUninstallConfirmId] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(true);
  const logsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (marketplaceOpen) {
      loadPlugins();
    }
  }, [marketplaceOpen, loadPlugins]);

  // Auto-scroll logs to bottom
  useEffect(() => {
    if (installProgress?.logs && showLogs) {
      logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [installProgress?.logs, showLogs]);

  if (!marketplaceOpen) return null;

  const handleInstall = async (source: string) => {
    if (!source.trim() || installing) return;
    try {
      await installPlugin(source.trim());
      setSourceInput("");
    } catch {
      // Handled in store and displayed in live progress drawer
    }
  };

  const pendingUpdatesList = Object.values(availableUpdates).filter((u) => u.has_update);
  const pendingUpdatesCount = pendingUpdatesList.length;

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
                <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-[var(--surface-hover)] border border-[var(--border)] text-zinc-300">
                  <PuzzleIcon size={20} />
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
                    Extensii modulare independente &bull; Suport complet pentru orice repo/fork GitHub
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => checkForUpdates()}
                  disabled={checkingUpdates || installing}
                  className="flex h-8 items-center gap-1.5 px-2.5 rounded-md border border-[var(--border)] bg-[var(--surface)] hover:bg-white/5 text-zinc-300 text-xs font-mono transition disabled:opacity-50 cursor-pointer"
                  title="Verifică dacă există versiuni noi pe GitHub"
                >
                  <RefreshIcon
                    size={12}
                    className={checkingUpdates ? "animate-spin text-cyan-400" : "text-zinc-400"}
                  />
                  <span className="hidden sm:inline">
                    {checkingUpdates ? "Verificare..." : "Verifică actualizări"}
                  </span>
                </button>

                <button
                  type="button"
                  onClick={closeMarketplace}
                  className="flex h-8 w-8 items-center justify-center rounded-md border border-[var(--border)] text-zinc-400 hover:bg-white/5 hover:text-white transition"
                >
                  <XIcon size={14} />
                </button>
              </div>
            </div>

            {/* Updates Banner if any pending */}
            {pendingUpdatesCount > 0 && (
              <div className="bg-gradient-to-r from-amber-500/10 via-cyan-500/10 to-transparent border-b border-amber-500/20 px-6 py-2.5 flex items-center justify-between gap-3 text-xs">
                <div className="flex items-center gap-2">
                  <span className="flex h-2 w-2 rounded-full bg-amber-400 animate-pulse" />
                  <span className="text-amber-200 font-medium">
                    Actualizări disponibile pentru {pendingUpdatesCount} {pendingUpdatesCount === 1 ? "plugin" : "plugin-uri"}
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => updateAllAvailablePlugins()}
                  disabled={installing}
                  className="h-6 px-3 bg-amber-400/20 hover:bg-amber-400/30 text-amber-300 border border-amber-400/40 rounded text-[11px] font-mono transition flex items-center gap-1.5 cursor-pointer disabled:opacity-50"
                >
                  <DownloadIcon size={11} />
                  <span>Actualizează toate</span>
                </button>
              </div>
            )}

            {/* Quick Install Bar */}
            <div className="border-b border-[var(--border)] bg-[var(--surface)] px-6 py-3.5 space-y-3">
              <div className="flex flex-col sm:flex-row gap-2.5 items-stretch">
                <div className="relative flex-1">
                  <TextInput
                    value={sourceInput}
                    onChange={(e: ChangeEvent<HTMLInputElement>) => setSourceInput(e.target.value)}
                    onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => e.key === "Enter" && handleInstall(sourceInput)}
                    placeholder="e.g. DemOnJR/xconsole-plugin-redis sau calea locală..."
                    className="h-9 text-xs font-mono w-full bg-[var(--surface-2)] border-[var(--border)] px-3 focus:border-cyan-500/80 rounded-md"
                  />
                </div>
                <button
                  type="button"
                  disabled={installing || !sourceInput.trim()}
                  onClick={() => handleInstall(sourceInput)}
                  className="h-9 px-4 text-xs font-medium bg-zinc-100 text-zinc-950 hover:bg-white active:scale-[0.98] transition-all flex items-center justify-center gap-2 rounded-md shrink-0 border-none cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed shadow-xs"
                >
                  {installing ? (
                    <>
                      <SpinnerIcon size={13} className="text-zinc-900" />
                      <span>Instalare…</span>
                    </>
                  ) : (
                    <>
                      <DownloadIcon size={13} className="text-zinc-900" />
                      <span>Instalează</span>
                    </>
                  )}
                </button>
              </div>

              {/* Real-time Live Progress Bar & Logs Console */}
              {installProgress && (
                <div className="rounded-lg border border-[var(--border-strong)] bg-zinc-950/70 p-3.5 space-y-2.5 animate-fadeIn">
                  <div className="flex items-center justify-between gap-3 text-xs">
                    <div className="flex items-center gap-2 min-w-0">
                      {installProgress.status === "installing" && (
                        <span className="relative flex h-2 w-2">
                          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-cyan-400 opacity-75"></span>
                          <span className="relative inline-flex rounded-full h-2 w-2 bg-cyan-500"></span>
                        </span>
                      )}
                      {installProgress.status === "success" && (
                        <div className="flex h-4 w-4 items-center justify-center rounded-full bg-emerald-500/20 text-emerald-400">
                          <CheckIcon size={10} />
                        </div>
                      )}
                      {installProgress.status === "error" && (
                        <div className="flex h-4 w-4 items-center justify-center rounded-full bg-red-500/20 text-red-400">
                          <XIcon size={10} />
                        </div>
                      )}

                      <span className="font-medium text-gray-200 truncate">
                        {installProgress.step}
                      </span>
                      {installProgress.stepIndex > 0 && installProgress.status === "installing" && (
                        <span className="text-[10px] font-mono text-zinc-500">
                          (Pasul {installProgress.stepIndex}/{installProgress.totalSteps})
                        </span>
                      )}
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                      <span className="font-mono text-xs text-cyan-400 font-semibold">
                        {installProgress.percent}%
                      </span>
                      <button
                        type="button"
                        onClick={() => setShowLogs(!showLogs)}
                        className="flex items-center gap-1 rounded bg-white/5 hover:bg-white/10 px-2 py-0.5 text-[10px] font-mono text-zinc-400 transition"
                      >
                        <TerminalIcon size={11} />
                        <span>{showLogs ? "Ascunde log" : "Afișează log"}</span>
                      </button>
                      {installProgress.status !== "installing" && (
                        <button
                          type="button"
                          onClick={clearInstallProgress}
                          className="text-zinc-500 hover:text-white p-0.5 transition"
                          title="Închide panoul de progres"
                        >
                          <XIcon size={12} />
                        </button>
                      )}
                    </div>
                  </div>

                  {/* Progress track */}
                  <div className="h-1.5 w-full bg-white/10 rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full transition-all duration-300 ease-out ${
                        installProgress.status === "error"
                          ? "bg-red-500"
                          : installProgress.status === "success"
                            ? "bg-emerald-500"
                            : "bg-gradient-to-r from-cyan-500 to-blue-500"
                      }`}
                      style={{ width: `${Math.max(5, installProgress.percent)}%` }}
                    />
                  </div>

                  {/* Terminal Log Output */}
                  {showLogs && installProgress.logs && installProgress.logs.length > 0 && (
                    <div className="mt-2 max-h-36 overflow-y-auto rounded border border-white/5 bg-black/60 p-2.5 font-mono text-[11px] leading-relaxed text-zinc-300 space-y-0.5">
                      {installProgress.logs.map((line, idx) => (
                        <div key={idx} className="break-all whitespace-pre-wrap flex gap-2">
                          <span className="text-zinc-600 select-none">&gt;</span>
                          <span className={line.toLowerCase().includes("error") || line.toLowerCase().includes("err") ? "text-red-400" : "text-zinc-300"}>
                            {line}
                          </span>
                        </div>
                      ))}
                      <div ref={logsEndRef} />
                    </div>
                  )}

                  {installProgress.status === "error" && installProgress.error && (
                    <div className="text-xs text-red-300 bg-red-950/40 border border-red-900/40 rounded px-2.5 py-1.5 flex items-center justify-between">
                      <span>{installProgress.error}</span>
                    </div>
                  )}

                  {installProgress.status === "success" && installProgress.pluginName && (
                    <div className="text-xs text-emerald-300 bg-emerald-950/30 border border-emerald-900/40 rounded px-2.5 py-1.5 flex items-center justify-between">
                      <span>Pluginul '{installProgress.pluginName}' este instalat și activat cu succes!</span>
                      <Button
                        variant="ghost"
                        className="text-[10px] py-0.5 px-2 text-emerald-300 hover:text-white border-emerald-800"
                        onClick={clearInstallProgress}
                      >
                        Gata
                      </Button>
                    </div>
                  )}
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
                    <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-white/5 border border-white/10 text-zinc-500 mb-3">
                      <PuzzleIcon size={24} />
                    </div>
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
                              <div className="flex items-center gap-3">
                                <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-[var(--surface-hover)] border border-[var(--border)] text-zinc-200 shrink-0">
                                  <PluginIcon icon={plugin.icon} pluginId={plugin.id} size={20} />
                                </div>
                                <div>
                                  <div className="flex items-center gap-1.5 flex-wrap">
                                    <h4 className="text-xs font-semibold text-gray-100 hover:text-white transition">
                                      {plugin.name}
                                    </h4>
                                    <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-zinc-400">
                                      v{plugin.version}
                                    </span>
                                    {availableUpdates[plugin.id]?.has_update && (
                                      <span className="rounded bg-amber-500/15 text-amber-300 border border-amber-500/30 px-1.5 py-0.2 text-[9px] font-mono animate-pulse">
                                        update: {availableUpdates[plugin.id].latest_commit || "nou"}
                                      </span>
                                    )}
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
                              {availableUpdates[plugin.id]?.has_update && (
                                <button
                                  type="button"
                                  disabled={installing || Boolean(updatingPluginIds[plugin.id])}
                                  onClick={() => updateSinglePlugin(plugin.id)}
                                  className="h-6 px-2.5 bg-amber-400/20 hover:bg-amber-400/30 text-amber-300 border border-amber-400/40 rounded text-[11px] font-mono transition flex items-center gap-1 cursor-pointer disabled:opacity-50"
                                >
                                  {updatingPluginIds[plugin.id] ? (
                                    <>
                                      <SpinnerIcon size={11} className="text-amber-300" />
                                      <span>Actualizare...</span>
                                    </>
                                  ) : (
                                    <>
                                      <DownloadIcon size={11} />
                                      <span>Update</span>
                                    </>
                                  )}
                                </button>
                              )}

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
                                    <XIcon size={11} />
                                  </Button>
                                </div>
                              ) : (
                                <button
                                  type="button"
                                  className="flex h-6 w-6 items-center justify-center rounded text-zinc-500 hover:text-red-400 hover:bg-white/5 transition"
                                  title="Dezinstalează plugin"
                                  onClick={() => setUninstallConfirmId(plugin.id)}
                                >
                                  <TrashIcon size={13} />
                                </button>
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
                            <div className="flex items-center gap-3">
                              <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-[var(--surface-hover)] border border-[var(--border)] text-zinc-200 shrink-0">
                                <PluginIcon icon={item.icon} pluginId={item.id} size={20} />
                              </div>
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
                              <span className="text-xs text-emerald-400 font-mono flex items-center gap-1">
                                <CheckIcon size={12} /> Instalat
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
                            <button
                              type="button"
                              className="h-7 px-3 text-xs bg-zinc-100 hover:bg-white text-zinc-950 font-medium rounded transition flex items-center gap-1.5 cursor-pointer disabled:opacity-50"
                              disabled={installing}
                              onClick={() => handleInstall(item.repository)}
                            >
                              <DownloadIcon size={12} />
                              <span>Instalează</span>
                            </button>
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
