import { useState, useEffect } from "react";
import { usePluginStore } from "../../stores/pluginStore";
import { Button, Card } from "../settings/ui";
import { AgentMarkdown } from "../agent/AgentMarkdown";

export function PluginDetailView({
  pluginId,
  onBack,
}: {
  pluginId: string;
  onBack: () => void;
}) {
  const {
    plugins,
    definitions,
    togglePlugin,
    uninstallPlugin,
    openPluginView,
    getPluginReadme,
    closeMarketplace,
  } = usePluginStore();

  const plugin = plugins.find((p) => p.id === pluginId);
  const def = definitions[pluginId];

  const [activeTab, setActiveTab] = useState<"readme" | "capabilities" | "manifest">("readme");
  const [readme, setReadme] = useState<string>("");
  const [loadingReadme, setLoadingReadme] = useState<boolean>(true);
  const [confirmUninstall, setConfirmUninstall] = useState<boolean>(false);
  const [uninstalling, setUninstalling] = useState<boolean>(false);

  const isEnabled = plugin?.enabled !== false;
  const hasView = Boolean(def?.renderView);

  useEffect(() => {
    let mounted = true;
    setLoadingReadme(true);
    getPluginReadme(pluginId)
      .then((content) => {
        if (mounted) {
          setReadme(content);
          setLoadingReadme(false);
        }
      })
      .catch(() => {
        if (mounted) {
          setReadme(
            `# ${plugin?.name || pluginId}\n\nDocumentația oficială se sincronizează de pe GitHub: [https://github.com/DemOnJR/${pluginId}](https://github.com/DemOnJR/${pluginId})`
          );
          setLoadingReadme(false);
        }
      });
    return () => {
      mounted = false;
    };
  }, [pluginId, plugin?.name, getPluginReadme]);

  if (!plugin) {
    return (
      <div className="flex flex-col items-center justify-center p-12 text-center">
        <p className="text-sm text-gray-400 mb-4">Pluginul nu a fost găsit.</p>
        <Button variant="ghost" onClick={onBack}>
          &larr; Înapoi la listă
        </Button>
      </div>
    );
  }

  const handleUninstall = async () => {
    setUninstalling(true);
    try {
      await uninstallPlugin(plugin.id);
      onBack();
    } catch {
      setUninstalling(false);
    }
  };

  const handleOpenView = () => {
    closeMarketplace();
    openPluginView(plugin.id);
  };

  const githubUrl = plugin.repository || `https://github.com/DemOnJR/${plugin.id}`;

  return (
    <div className="flex flex-col h-full overflow-hidden animate-in fade-in duration-150">
      {/* Top Breadcrumb & Controls */}
      <div className="flex items-center justify-between border-b border-[var(--border)] px-6 py-3 bg-[var(--surface-2)] shrink-0">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-1.5 text-xs text-gray-400 hover:text-indigo-400 transition font-medium"
        >
          <span>&larr;</span>
          <span>Înapoi la toate pluginurile</span>
        </button>

        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full ${
              isEnabled ? "bg-emerald-400 animate-pulse" : "bg-gray-600"
            }`}
          />
          <span className="text-[11px] font-mono text-gray-400">
            {isEnabled ? "ACTIV / ÎNCĂRCAT" : "DEZACTIVAT"}
          </span>
        </div>
      </div>

      {/* Main Details Body */}
      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        {/* Header Card */}
        <Card className="p-5 border border-[var(--border)] bg-[var(--surface-2)] shadow-sm">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="flex items-start gap-4">
              <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br from-indigo-500/20 to-purple-600/30 border border-indigo-500/30 text-3xl shrink-0 shadow-inner">
                {plugin.icon || "🧩"}
              </div>

              <div className="space-y-1">
                <div className="flex items-center gap-2 flex-wrap">
                  <h3 className="text-base font-bold text-gray-100">{plugin.name}</h3>
                  <span className="rounded bg-indigo-500/20 text-indigo-300 border border-indigo-500/30 px-2 py-0.5 text-[10px] font-mono font-medium">
                    v{plugin.version}
                  </span>
                  <span className="rounded bg-white/5 text-gray-400 px-2 py-0.5 text-[10px] font-mono">
                    {plugin.category}
                  </span>
                </div>

                <p className="text-xs text-gray-300 leading-relaxed max-w-xl">
                  {plugin.description}
                </p>

                <div className="flex items-center gap-3 text-[11px] text-gray-400 pt-1 font-mono">
                  <span>Autor: <strong className="text-gray-200">{plugin.author}</strong></span>
                  <span>&bull;</span>
                  <a
                    href={githubUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="text-indigo-400 hover:underline flex items-center gap-1"
                  >
                    <span>GitHub Repo</span>
                    <span>↗</span>
                  </a>
                </div>
              </div>
            </div>

            {/* Actions Bar */}
            <div className="flex flex-wrap md:flex-col items-end gap-2 shrink-0">
              <div className="flex items-center gap-2">
                <span className="text-xs text-gray-400">
                  {isEnabled ? "Activ" : "Dezactivat"}
                </span>
                <label className="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    checked={isEnabled}
                    onChange={() => togglePlugin(plugin.id, !isEnabled)}
                    className="sr-only peer"
                  />
                  <div className="w-10 h-5 bg-gray-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
                </label>
              </div>

              <div className="flex items-center gap-2">
                {hasView && (
                  <Button
                    variant="primary"
                    onClick={handleOpenView}
                    disabled={!isEnabled}
                    className="text-xs bg-indigo-600 hover:bg-indigo-500"
                  >
                    Deschide View &rarr;
                  </Button>
                )}

                {confirmUninstall ? (
                  <div className="flex items-center gap-1">
                    <Button
                      variant="danger"
                      onClick={handleUninstall}
                      disabled={uninstalling}
                      className="text-xs py-1 px-2"
                    >
                      {uninstalling ? "Se șterge…" : "Confirmă Ștergerea"}
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() => setConfirmUninstall(false)}
                      className="text-xs py-1 px-2"
                    >
                      Anulează
                    </Button>
                  </div>
                ) : (
                  <Button
                    variant="ghost"
                    onClick={() => setConfirmUninstall(true)}
                    className="text-xs text-red-400 hover:text-red-300 hover:bg-red-950/30"
                  >
                    🗑️ Dezinstalează
                  </Button>
                )}
              </div>
            </div>
          </div>
        </Card>

        {/* Tab Navigation */}
        <div className="flex border-b border-[var(--border)]">
          <button
            type="button"
            onClick={() => setActiveTab("readme")}
            className={`px-4 py-2.5 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "readme"
                ? "border-indigo-500 text-indigo-400"
                : "border-transparent text-gray-400 hover:text-gray-200"
            }`}
          >
            <span>📖</span>
            <span>Documentație &amp; README</span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("capabilities")}
            className={`px-4 py-2.5 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "capabilities"
                ? "border-indigo-500 text-indigo-400"
                : "border-transparent text-gray-400 hover:text-gray-200"
            }`}
          >
            <span>⚡</span>
            <span>Capacități &amp; Unelte AI</span>
            {plugin.capabilities?.agentTools && (
              <span className="rounded-full bg-indigo-500/20 text-indigo-300 px-1.5 py-0.2 text-[9px]">
                {(plugin.capabilities.agentTools as any[]).length}
              </span>
            )}
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("manifest")}
            className={`px-4 py-2.5 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "manifest"
                ? "border-indigo-500 text-indigo-400"
                : "border-transparent text-gray-400 hover:text-gray-200"
            }`}
          >
            <span>📦</span>
            <span>Configurație &amp; Manifest</span>
          </button>
        </div>

        {/* Tab 1: README Content */}
        {activeTab === "readme" && (
          <div className="rounded-xl border border-[var(--border)] bg-[var(--surface-2)]/60 p-6">
            {loadingReadme ? (
              <div className="flex items-center justify-center p-8 text-gray-500 text-xs">
                <span className="animate-spin mr-2">⠋</span> Se încarcă documentația pluginului…
              </div>
            ) : (
              <div className="prose prose-invert max-w-none text-xs leading-relaxed">
                <AgentMarkdown content={readme} />
              </div>
            )}
          </div>
        )}

        {/* Tab 2: Capabilities & Tools */}
        {activeTab === "capabilities" && (
          <div className="space-y-4">
            {plugin.capabilities?.agentTools && (plugin.capabilities.agentTools as any[]).length > 0 ? (
              <div className="space-y-2">
                <h4 className="text-xs font-semibold text-gray-200 uppercase tracking-wider">
                  Unelte AI Înregistrate (Agent Tools)
                </h4>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  {(plugin.capabilities.agentTools as any[]).map((tool) => (
                    <Card key={tool.name} className="p-3 border border-[var(--border)] bg-[var(--surface-2)]">
                      <div className="flex items-center justify-between mb-1">
                        <code className="text-xs font-bold text-cyan-300 font-mono">
                          {tool.name}
                        </code>
                        <span className="text-[9px] rounded bg-cyan-950/50 text-cyan-400 border border-cyan-500/20 px-1.5 py-0.2 font-mono">
                          tool
                        </span>
                      </div>
                      <p className="text-[11px] text-gray-400">{tool.description}</p>
                    </Card>
                  ))}
                </div>
              </div>
            ) : (
              <div className="p-4 text-xs text-gray-500 border border-dashed border-[var(--border)] rounded-xl text-center">
                Acest plugin nu expune unelte AI suplimentare.
              </div>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 pt-2">
              <Card className="p-3 border border-[var(--border)] bg-[var(--surface-2)]">
                <h5 className="text-[11px] font-semibold text-gray-300 mb-1">Interfață Utilizator</h5>
                <p className="text-[11px] text-gray-400">
                  {hasView
                    ? "✓ Include fereastră grafică dedicată (Modal View)"
                    : "○ Nu include fereastră modală"}
                </p>
              </Card>

              <Card className="p-3 border border-[var(--border)] bg-[var(--surface-2)]">
                <h5 className="text-[11px] font-semibold text-gray-300 mb-1">Nod de Canvas</h5>
                <p className="text-[11px] text-gray-400">
                  {plugin.capabilities?.canvasNode
                    ? `✓ Randare dinamică canvas (${(plugin.capabilities.canvasNode as any).type || "node"})`
                    : "○ Fără nod de canvas"}
                </p>
              </Card>
            </div>
          </div>
        )}

        {/* Tab 3: Manifest JSON & Tech Info */}
        {activeTab === "manifest" && (
          <div className="space-y-4 font-mono text-xs">
            <Card className="p-4 border border-[var(--border)] bg-[var(--surface-2)] space-y-2">
              <div className="text-gray-400">
                <strong className="text-gray-200">ID Plugin:</strong> {plugin.id}
              </div>
              <div className="text-gray-400">
                <strong className="text-gray-200">Cale Instalare:</strong>{" "}
                <span className="text-indigo-300">{plugin.installedPath || "~/.xconsole/plugins/" + plugin.id}</span>
              </div>
              <div className="text-gray-400">
                <strong className="text-gray-200">Entry Point:</strong> dist/index.js (ES Module)
              </div>
            </Card>

            <div className="rounded-xl border border-[var(--border)] bg-black/40 p-4 overflow-x-auto text-[11px] text-gray-300">
              <pre>{JSON.stringify(plugin, null, 2)}</pre>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
