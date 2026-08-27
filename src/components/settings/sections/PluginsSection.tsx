import { useState, type ChangeEvent, type KeyboardEvent } from "react";
import { usePluginStore } from "../../../stores/pluginStore";
import { Button, Card, TextInput } from "../ui";
import { PluginDetailView } from "../../plugins/PluginDetailView";

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
  } = usePluginStore();

  const [source, setSource] = useState("");
  const [installError, setInstallError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [uninstallConfirmId, setUninstallConfirmId] = useState<string | null>(null);

  const handleInstall = async () => {
    if (!source.trim()) return;
    setInstallError(null);
    setSuccess(null);
    try {
      const p = await installPlugin(source.trim());
      setSource("");
      setSuccess(`Plugin '${p.name}' installed successfully.`);
      setTimeout(() => setSuccess(null), 4000);
    } catch (e) {
      setInstallError(String(e));
    }
  };

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

        <Button
          variant="primary"
          className="bg-zinc-100 hover:bg-white text-zinc-950 text-xs font-medium border-none px-3 py-1.5"
          onClick={openMarketplace}
        >
          Marketplace &rarr;
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto p-6 space-y-4">
        {/* Quick install box */}
        <Card className="p-4 bg-[var(--surface-2)] border-[var(--border)]">
          <h4 className="text-xs font-semibold text-gray-200 mb-1">
            Install Plugin
          </h4>
          <p className="text-[11px] text-zinc-400 mb-3">
            Enter GitHub repo (e.g. <code>DemOnJR/xconsole-plugin-redis</code>) or local path:
          </p>
          <div className="flex gap-2">
            <TextInput
              value={source}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setSource(e.target.value)}
              onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => e.key === "Enter" && handleInstall()}
              placeholder="e.g. DemOnJR/xconsole-plugin-redis"
              className="text-xs font-mono flex-1 bg-[var(--surface)] border-[var(--border)]"
            />
            <Button
              variant="primary"
              className="bg-zinc-100 hover:bg-white text-zinc-950 text-xs font-medium whitespace-nowrap px-3 py-1.5 border-none"
              disabled={installing || !source.trim()}
              onClick={handleInstall}
            >
              {installing ? "Installing…" : "Install"}
            </Button>
          </div>

          {installError && (
            <p className="mt-2 text-xs text-red-400 bg-red-950/40 border border-red-900/40 rounded p-2">
              ❌ {installError}
            </p>
          )}
          {success && (
            <p className="mt-2 text-xs text-emerald-300 bg-zinc-900 border border-emerald-900/50 rounded p-2">
              ✓ {success}
            </p>
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
                    <div className="flex items-center gap-2">
                      <span className="text-lg">{plugin.icon || "🧩"}</span>
                      <span className="font-semibold text-xs text-gray-100 hover:text-white transition">
                        {plugin.name}
                      </span>
                      <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-zinc-400">
                        v{plugin.version}
                      </span>
                      <span className="rounded bg-white/5 text-zinc-400 px-1.5 py-0.2 text-[9px] uppercase font-mono">
                        {plugin.category}
                      </span>
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
                          ✕
                        </Button>
                      </div>
                    ) : (
                      <Button
                        variant="ghost"
                        className="text-xs text-zinc-500 hover:text-red-400 px-1.5 py-1"
                        title="Uninstall plugin"
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
      </div>
    </div>
  );
}
