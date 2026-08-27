import { useState, type ChangeEvent, type KeyboardEvent } from "react";
import { usePluginStore } from "../../../stores/pluginStore";
import { Button, Card, TextInput } from "../ui";

export function PluginsSection() {
  const {
    plugins,
    installPlugin,
    uninstallPlugin,
    togglePlugin,
    openMarketplace,
    installing,
    openPluginView,
  } = usePluginStore();

  const [source, setSource] = useState("");
  const [installError, setInstallError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const handleInstall = async () => {
    if (!source.trim()) return;
    setInstallError(null);
    setSuccess(null);
    try {
      const p = await installPlugin(source.trim());
      setSource("");
      setSuccess(`Pluginul '${p.name}' a fost instalat cu succes!`);
      setTimeout(() => setSuccess(null), 4000);
    } catch (e) {
      setInstallError(String(e));
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-[var(--border)] px-6 py-4">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-gray-100">
              Pluginuri &amp; Extensii Modulare (Harness)
            </h3>
            <span className="rounded bg-indigo-500/20 text-indigo-300 border border-indigo-500/30 px-1.5 py-0.2 text-[10px] font-mono">
              xConsole Engine
            </span>
          </div>
          <p className="text-xs text-gray-400 mt-0.5">
            Arhitectură de pluginuri spatiotemporal composable: extinde xConsole fără să îngreunezi nucleul aplicației.
          </p>
        </div>

        <Button
          variant="primary"
          className="bg-indigo-600 hover:bg-indigo-500 text-white text-xs border-none"
          onClick={openMarketplace}
        >
          🌐 Deschide Marketplace &rarr;
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto p-6 space-y-4">
        {/* Quick install box */}
        <Card className="p-4 bg-[var(--surface-2)]">
          <h4 className="text-xs font-semibold text-gray-200 mb-1">
            Instalează un plugin nou
          </h4>
          <p className="text-[11px] text-gray-400 mb-3">
            Lipește un URL de GitHub (e.g. <code>xconsole-plugins/xconsole-plugin-cloudflare</code>) sau calea către un folder local:
          </p>
          <div className="flex gap-2">
            <TextInput
              value={source}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setSource(e.target.value)}
              onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => e.key === "Enter" && handleInstall()}
              placeholder="e.g. xconsole-plugins/xconsole-plugin-redis"
              className="text-xs font-mono flex-1"
            />
            <Button
              variant="primary"
              className="bg-indigo-600 hover:bg-indigo-500 text-white text-xs whitespace-nowrap border-none"
              disabled={installing || !source.trim()}
              onClick={handleInstall}
            >
              {installing ? "Instalare…" : "🚀 Instalează"}
            </Button>
          </div>

          {installError && (
            <p className="mt-2 text-xs text-red-400 bg-red-950/40 border border-red-800/40 rounded p-2">
              ❌ {installError}
            </p>
          )}
          {success && (
            <p className="mt-2 text-xs text-emerald-400 bg-emerald-950/40 border border-emerald-800/40 rounded p-2">
              ✓ {success}
            </p>
          )}
        </Card>

        {/* Installed plugins list */}
        <div className="space-y-3">
          <h4 className="text-xs font-semibold text-gray-300">
            Pluginuri active ({plugins.length})
          </h4>

          {plugins.map((plugin) => {
            const isEnabled = plugin.enabled !== false;

            return (
              <Card
                key={plugin.id}
                className={`p-4 transition border ${
                  isEnabled
                    ? "border-[var(--border)] bg-[var(--surface-2)]"
                    : "border-white/5 bg-white/[0.02] opacity-60"
                }`}
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="space-y-1.5 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-xl">{plugin.icon}</span>
                      <span className="font-semibold text-xs text-gray-100">
                        {plugin.name}
                      </span>
                      <span className="rounded bg-white/10 px-1.5 py-0.2 text-[9px] font-mono text-gray-400">
                        v{plugin.version}
                      </span>
                      <span className="rounded bg-indigo-500/10 text-indigo-400 px-1.5 py-0.2 text-[9px] uppercase">
                        {plugin.category}
                      </span>
                    </div>

                    <p className="text-xs text-gray-300">
                      {plugin.description}
                    </p>

                    <div className="text-[10px] text-gray-500 font-mono">
                      ID: {plugin.id} &bull; Autor: {plugin.author}
                    </div>
                  </div>

                  <div className="flex items-center gap-3 shrink-0">
                    {plugin.capabilities?.navItem && isEnabled && (
                      <Button
                        variant="ghost"
                        className="text-xs text-indigo-400 hover:text-indigo-300 px-2 py-1"
                        onClick={() => openPluginView(plugin.id)}
                      >
                        Deschide View &rarr;
                      </Button>
                    )}

                    <label className="relative inline-flex items-center cursor-pointer">
                      <input
                        type="checkbox"
                        checked={isEnabled}
                        onChange={() => togglePlugin(plugin.id, !isEnabled)}
                        className="sr-only peer"
                      />
                      <div className="w-9 h-5 bg-gray-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
                    </label>

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
                        Șterge
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
