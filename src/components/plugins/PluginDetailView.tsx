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
            `# ${plugin?.name || pluginId}\n\nDocumentation is available on GitHub: [https://github.com/DemOnJR/${pluginId}](https://github.com/DemOnJR/${pluginId})`
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
      <div className="flex flex-col items-center justify-center p-12 text-center font-sans">
        <p className="text-xs text-zinc-400 mb-4">Plugin not found.</p>
        <Button variant="ghost" onClick={onBack} className="text-xs border border-[var(--border)]">
          &larr; Back to plugins
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
    <div className="flex flex-col h-full overflow-hidden font-sans">
      {/* Top Breadcrumb & Controls */}
      <div className="flex items-center justify-between border-b border-[var(--border)] px-6 py-2.5 bg-[var(--surface-2)] shrink-0">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-1.5 text-xs text-zinc-400 hover:text-zinc-100 transition font-mono"
        >
          <span>&larr;</span>
          <span>back to plugins</span>
        </button>

        <div className="flex items-center gap-2">
          <span
            className={`h-1.5 w-1.5 rounded-full ${
              isEnabled ? "bg-emerald-400" : "bg-zinc-600"
            }`}
          />
          <span className="text-[10px] font-mono text-zinc-400 uppercase">
            {isEnabled ? "active" : "disabled"}
          </span>
        </div>
      </div>

      {/* Main Details Body */}
      <div className="flex-1 overflow-y-auto p-6 space-y-5">
        {/* Header Card */}
        <Card className="p-4 border border-[var(--border)] bg-[var(--surface-2)]">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="flex items-start gap-3.5">
              <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-[var(--surface-hover)] border border-[var(--border)] text-2xl shrink-0">
                {plugin.icon || "🧩"}
              </div>

              <div className="space-y-1">
                <div className="flex items-center gap-2 flex-wrap">
                  <h3 className="text-sm font-semibold text-gray-100">{plugin.name}</h3>
                  <span className="rounded bg-white/10 text-zinc-300 border border-white/10 px-1.5 py-0.2 text-[9px] font-mono">
                    v{plugin.version}
                  </span>
                  <span className="rounded bg-white/5 text-zinc-400 px-1.5 py-0.2 text-[9px] font-mono">
                    {plugin.category}
                  </span>
                </div>

                <p className="text-xs text-zinc-400 leading-relaxed max-w-xl">
                  {plugin.description}
                </p>

                <div className="flex items-center gap-3 text-[11px] text-zinc-500 pt-0.5 font-mono">
                  <span>author: <strong className="text-zinc-300">{plugin.author}</strong></span>
                  <span>&bull;</span>
                  <a
                    href={githubUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="text-zinc-400 hover:text-white underline underline-offset-2 flex items-center gap-1"
                  >
                    <span>GitHub</span>
                    <span>↗</span>
                  </a>
                </div>
              </div>
            </div>

            {/* Actions Bar */}
            <div className="flex flex-wrap md:flex-col items-end gap-2 shrink-0">
              <div className="flex items-center gap-2">
                <span className="text-[11px] text-zinc-500 font-mono">
                  {isEnabled ? "enabled" : "disabled"}
                </span>
                <label className="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    checked={isEnabled}
                    onChange={() => togglePlugin(plugin.id, !isEnabled)}
                    className="sr-only peer"
                  />
                  <div className="w-8 h-4.5 bg-zinc-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-zinc-950 after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-zinc-200 peer-checked:after:bg-zinc-900"></div>
                </label>
              </div>

              <div className="flex items-center gap-2">
                {hasView && (
                  <Button
                    variant="primary"
                    onClick={handleOpenView}
                    disabled={!isEnabled}
                    className="text-xs bg-zinc-100 hover:bg-white text-zinc-950 font-medium px-3 py-1"
                  >
                    Open View &rarr;
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
                      {uninstalling ? "Uninstalling…" : "Confirm"}
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() => setConfirmUninstall(false)}
                      className="text-xs py-1 px-2"
                    >
                      Cancel
                    </Button>
                  </div>
                ) : (
                  <Button
                    variant="ghost"
                    onClick={() => setConfirmUninstall(true)}
                    className="text-xs text-zinc-500 hover:text-red-400"
                  >
                    🗑️ Uninstall
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
            className={`px-3 py-2 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "readme"
                ? "border-zinc-200 text-white"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <span>README.md</span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("capabilities")}
            className={`px-3 py-2 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "capabilities"
                ? "border-zinc-200 text-white"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <span>Capabilities</span>
            {plugin.capabilities?.agentTools && (
              <span className="rounded bg-zinc-800 text-zinc-300 px-1.5 py-0.2 text-[9px] font-mono">
                {(plugin.capabilities.agentTools as any[]).length}
              </span>
            )}
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("manifest")}
            className={`px-3 py-2 text-xs font-medium border-b-2 transition flex items-center gap-1.5 ${
              activeTab === "manifest"
                ? "border-zinc-200 text-white"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <span>Manifest</span>
          </button>
        </div>

        {/* Tab 1: README Content */}
        {activeTab === "readme" && (
          <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)]/60 p-5">
            {loadingReadme ? (
              <div className="flex items-center justify-center p-8 text-zinc-500 text-xs font-mono">
                <span className="animate-spin mr-2">⠋</span> loading documentation…
              </div>
            ) : (
              <div className="prose prose-invert max-w-none text-xs leading-relaxed font-sans">
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
                <h4 className="text-xs font-semibold text-zinc-300 font-mono">
                  AGENT TOOLS
                </h4>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-2.5">
                  {(plugin.capabilities.agentTools as any[]).map((tool) => (
                    <Card key={tool.name} className="p-3 border border-[var(--border)] bg-[var(--surface-2)]">
                      <div className="flex items-center justify-between mb-1">
                        <code className="text-xs font-mono font-bold text-zinc-200">
                          {tool.name}
                        </code>
                        <span className="text-[9px] rounded bg-zinc-800 text-zinc-400 border border-zinc-700 px-1.5 py-0.2 font-mono">
                          tool
                        </span>
                      </div>
                      <p className="text-[11px] text-zinc-400">{tool.description}</p>
                    </Card>
                  ))}
                </div>
              </div>
            ) : (
              <div className="p-4 text-xs text-zinc-500 border border-dashed border-[var(--border)] rounded-lg text-center font-mono">
                No agent tools exported.
              </div>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 gap-2.5 pt-1">
              <Card className="p-3 border border-[var(--border)] bg-[var(--surface-2)]">
                <h5 className="text-xs font-semibold text-zinc-300 mb-1 font-mono">UI VIEW</h5>
                <p className="text-[11px] text-zinc-400">
                  {hasView
                    ? "✓ Includes dedicated interactive modal view"
                    : "○ No modal view"}
                </p>
              </Card>

              <Card className="p-3 border border-[var(--border)] bg-[var(--surface-2)]">
                <h5 className="text-xs font-semibold text-zinc-300 mb-1 font-mono">CANVAS NODE</h5>
                <p className="text-[11px] text-zinc-400">
                  {plugin.capabilities?.canvasNode
                    ? `✓ Dynamic canvas node (${(plugin.capabilities.canvasNode as any).type || "node"})`
                    : "○ No canvas node"}
                </p>
              </Card>
            </div>
          </div>
        )}

        {/* Tab 3: Manifest JSON & Tech Info */}
        {activeTab === "manifest" && (
          <div className="space-y-3 font-mono text-xs">
            <Card className="p-3 border border-[var(--border)] bg-[var(--surface-2)] space-y-1.5">
              <div className="text-zinc-400">
                <strong className="text-zinc-200">ID:</strong> {plugin.id}
              </div>
              <div className="text-zinc-400">
                <strong className="text-zinc-200">Path:</strong>{" "}
                <span className="text-zinc-300">{plugin.installedPath || "~/.xconsole/plugins/" + plugin.id}</span>
              </div>
              <div className="text-zinc-400">
                <strong className="text-zinc-200">Entry:</strong> dist/index.js (ES Module)
              </div>
            </Card>

            <div className="rounded-lg border border-[var(--border)] bg-black/50 p-3 overflow-x-auto text-[11px] text-zinc-400">
              <pre>{JSON.stringify(plugin, null, 2)}</pre>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
