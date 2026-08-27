import { useState, useRef, useEffect, type ChangeEvent, type KeyboardEvent } from "react";
import { usePluginStore } from "../../../stores/pluginStore";
import { Button, Card, TextInput } from "../ui";
import { PluginDetailView } from "../../plugins/PluginDetailView";
import { PluginIcon } from "../../plugins/PluginIcon";
import {
  PuzzleIcon,
  SpinnerIcon,
  CheckIcon,
  XIcon,
  TrashIcon,
  DownloadIcon,
  TerminalIcon,
  RefreshIcon,
} from "../../icons";

export function PluginsSection() {
  const {
    plugins,
    definitions,
    installPlugin,
    uninstallPlugin,
    togglePlugin,
    openMarketplace,
    selectedPluginId,
    selectPlugin,
    installing,
    openPluginView,
    installProgress,
    clearInstallProgress,
    availableUpdates,
    checkingUpdates,
    updatingPluginIds,
    autoCheckUpdates,
    autoUpdateEnabled,
    checkForUpdates,
    updateSinglePlugin,
    updateAllAvailablePlugins,
    setAutoCheckUpdates,
    setAutoUpdateEnabled,
  } = usePluginStore();

  const [source, setSource] = useState("");
  const [uninstallConfirmId, setUninstallConfirmId] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(true);
  const logsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (installProgress?.logs && showLogs) {
      logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [installProgress?.logs, showLogs]);

  const handleInstall = async () => {
    if (!source.trim() || installing) return;
    try {
      await installPlugin(source.trim());
      setSource("");
    } catch {
      // Handled in store and progress drawer
    }
  };

  const pendingUpdatesList = Object.values(availableUpdates).filter((u) => u.has_update);
  const pendingUpdatesCount = pendingUpdatesList.length;

  if (selectedPluginId) {
    return (
      <div className="flex h-full flex-col">
        <PluginDetailView
          pluginId={selectedPluginId}
          onBack={() => selectPlugin(null)}
        />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col font-sans">
      <div className="flex items-center justify-between border-b border-[var(--border)] px-6 py-3.5">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-gray-100">
              Plugins &amp; Extensions
            </h3>
            <span className="rounded bg-white/10 text-zinc-300 border border-white/10 px-1.5 py-0.2 text-[10px] font-mono">
              Harness Core
            </span>
          </div>
          <p className="text-xs text-zinc-400 mt-0.5">
            Microkernel architecture — modular, decoupled extensions for xConsole.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => checkForUpdates()}
            disabled={checkingUpdates || installing}
            className="flex h-8 items-center gap-1.5 px-2.5 rounded-md border border-[var(--border)] bg-[var(--surface)] hover:bg-white/5 text-zinc-300 text-xs font-mono transition disabled:opacity-50 cursor-pointer"
            title="Verifică actualizări pe GitHub"
          >
            <RefreshIcon
              size={12}
              className={checkingUpdates ? "animate-spin text-cyan-400" : "text-zinc-400"}
            />
            <span className="hidden sm:inline">
              {checkingUpdates ? "Verificare..." : "Verifică actualizări"}
            </span>
          </button>

          <Button
            variant="primary"
            className="bg-zinc-100 hover:bg-white text-zinc-950 text-xs font-medium border-none px-3 py-1.5 flex items-center gap-1.5"
            onClick={openMarketplace}
          >
            <PuzzleIcon size={14} />
            <span>Marketplace &rarr;</span>
          </Button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6 space-y-4">
        {/* Update & Fork Preferences Card */}
        <Card className="p-4 bg-[var(--surface-2)] border-[var(--border)] space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h4 className="text-xs font-semibold text-gray-200">
                Sistem Actualizări &amp; Git Forks
              </h4>
              <p className="text-[11px] text-zinc-400 mt-0.5">
                Setări pentru verificarea automată și instalarea versiunilor noi din GitHub/fork-uri.
              </p>
            </div>

            {pendingUpdatesCount > 0 && (
              <button
                type="button"
                onClick={() => updateAllAvailablePlugins()}
                disabled={installing}
                className="h-7 px-3 bg-amber-400/20 hover:bg-amber-400/30 text-amber-300 border border-amber-400/40 rounded text-xs font-mono transition flex items-center gap-1.5 cursor-pointer disabled:opacity-50"
              >
                <DownloadIcon size={12} />
                <span>Actualizează tot ({pendingUpdatesCount})</span>
              </button>
            )}
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-3 pt-1">
            <div className="flex items-center justify-between p-2.5 rounded-lg border border-[var(--border)] bg-[var(--surface)]">
              <div>
                <div className="text-xs font-medium text-gray-200">Verificare automată</div>
                <div className="text-[10px] text-zinc-500">Verifică versiuni noi pe GitHub la pornire</div>
              </div>
              <label className="relative inline-flex items-center cursor-pointer">
                <input
                  type="checkbox"
                  checked={autoCheckUpdates}
                  onChange={(e) => setAutoCheckUpdates(e.target.checked)}
                  className="sr-only peer"
                />
                <div className="w-8 h-4.5 bg-zinc-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-zinc-950 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-zinc-200 peer-checked:after:bg-zinc-900"></div>
              </label>
            </div>

            <div className="flex items-center justify-between p-2.5 rounded-lg border border-[var(--border)] bg-[var(--surface)]">
              <div>
                <div className="text-xs font-medium text-gray-200">Actualizare automată</div>
                <div className="text-[10px] text-zinc-500">Instalează silențios noile versiuni în fundal</div>
              </div>
              <label className="relative inline-flex items-center cursor-pointer">
                <input
                  type="checkbox"
                  checked={autoUpdateEnabled}
                  onChange={(e) => setAutoUpdateEnabled(e.target.checked)}
                  className="sr-only peer"
                />
                <div className="w-8 h-4.5 bg-zinc-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-zinc-950 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-zinc-200 peer-checked:after:bg-zinc-900"></div>
              </label>
            </div>
          </div>
        </Card>

        {/* Quick install box */}
        <Card className="p-4 bg-[var(--surface-2)] border-[var(--border)] space-y-3">
          <div>
            <h4 className="text-xs font-semibold text-gray-200 mb-1">
              Instalează Plugin
            </h4>
            <p className="text-[11px] text-zinc-400">
              Introdu adresa GitHub (e.g. <code>DemOnJR/xconsole-plugin-redis</code> sau contul/fork-ul tău):
            </p>
          </div>

          <div className="flex gap-2">
            <TextInput
              value={source}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setSource(e.target.value)}
              onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => e.key === "Enter" && handleInstall()}
              placeholder="e.g. DemOnJR/xconsole-plugin-redis sau username/my-forked-plugin"
              className="h-9 text-xs font-mono flex-1 bg-[var(--surface)] border-[var(--border)] px-3 rounded-md"
            />
            <button
              type="button"
              className="h-9 px-4 text-xs font-medium bg-zinc-100 hover:bg-white text-zinc-950 rounded-md transition flex items-center gap-1.5 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed border-none shrink-0"
              disabled={installing || !source.trim()}
              onClick={handleInstall}
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

          {/* Live Progress Bar & Terminal Logs */}
          {installProgress && (
            <div className="rounded-lg border border-[var(--border-strong)] bg-zinc-950/70 p-3.5 space-y-2.5 animate-fadeIn mt-3">
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
                      title="Închide panoul"
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
                  <span>Pluginul '{installProgress.pluginName}' este gata!</span>
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
        </Card>

        {/* Installed plugins list */}
        <div className="space-y-3">
          <h4 className="text-xs font-semibold text-zinc-300 font-mono">
            INSTALLED PLUGINS ({plugins.length})
          </h4>

          {plugins.map((plugin) => {
            const isEnabled = plugin.enabled !== false;
            const def = definitions[plugin.id];
            const hasView = Boolean(def?.renderView);
            const hasUpdate = Boolean(availableUpdates[plugin.id]?.has_update);
            const isUpdating = Boolean(updatingPluginIds[plugin.id]);

            return (
              <Card
                key={plugin.id}
                className={`p-4 transition border cursor-pointer ${
                  isEnabled
                    ? "border-[var(--border)] bg-[var(--surface-2)] hover:border-zinc-500"
                    : "border-white/5 bg-white/[0.02] opacity-55"
                }`}
                onClick={() => selectPlugin(plugin.id)}
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="space-y-1.5 flex-1">
                    <div className="flex items-center gap-2.5 flex-wrap">
                      <div className="flex h-7 w-7 items-center justify-center rounded bg-[var(--surface-hover)] border border-[var(--border)] text-zinc-200 shrink-0">
                        <PluginIcon icon={plugin.icon} pluginId={plugin.id} size={16} />
                      </div>
                      <span className="font-semibold text-xs text-gray-100 hover:text-white transition">
                        {plugin.name}
                      </span>
                      <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-zinc-400">
                        v{plugin.version}
                      </span>
                      <span className="rounded bg-white/5 text-zinc-400 px-1.5 py-0.2 text-[9px] uppercase font-mono">
                        {plugin.category}
                      </span>
                      {hasUpdate && (
                        <span className="rounded bg-amber-500/15 text-amber-300 border border-amber-500/30 px-1.5 py-0.2 text-[9px] font-mono animate-pulse">
                          update disponibil ({availableUpdates[plugin.id].latest_commit || "nou"})
                        </span>
                      )}
                    </div>

                    <p className="text-xs text-zinc-400 line-clamp-2">
                      {plugin.description}
                    </p>

                    <div className="text-[10px] text-zinc-500 font-mono">
                      id: {plugin.id} &bull; author: {plugin.author}
                    </div>
                  </div>

                  <div
                    className="flex items-center gap-2 shrink-0"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {hasUpdate && (
                      <button
                        type="button"
                        disabled={installing || isUpdating}
                        onClick={() => updateSinglePlugin(plugin.id)}
                        className="h-6 px-2.5 bg-amber-400/20 hover:bg-amber-400/30 text-amber-300 border border-amber-400/40 rounded text-[11px] font-mono transition flex items-center gap-1 cursor-pointer disabled:opacity-50"
                      >
                        {isUpdating ? (
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

                    <Button
                      variant="ghost"
                      className="text-xs text-zinc-300 hover:text-white px-2 py-1"
                      onClick={() => selectPlugin(plugin.id)}
                    >
                      docs
                    </Button>

                    {hasView && isEnabled && (
                      <Button
                        variant="ghost"
                        className="text-xs text-zinc-200 hover:text-white px-2 py-1 border border-[var(--border)]"
                        onClick={() => openPluginView(plugin.id)}
                      >
                        Open View
                      </Button>
                    )}

                    {/* Toggle Switch */}
                    <div onClick={(e) => e.stopPropagation()}>
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
                        title="Uninstall plugin"
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
      </div>
    </div>
  );
}

