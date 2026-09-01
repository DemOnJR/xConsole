import { memo, Suspense } from "react";
import { usePluginStore } from "../../stores/pluginStore";

export const DynamicPluginNode = memo(function DynamicPluginNode({
  id,
  data,
  type,
  selected,
}: any) {
  const plugins = usePluginStore((s) => s.plugins);
  const definitions = usePluginStore((s) => s.definitions);

  const matchedPlugin = plugins.find(
    (p) =>
      p.id === type ||
      p.id === `xconsole-plugin-${type}` ||
      (p.id === "xconsole-plugin-database" && (type === "db" || type === "database")) ||
      (p.id === "xconsole-plugin-sftp" && (type === "sftp" || type === "ftp")) ||
      (p.id === "xconsole-plugin-agent" && (type === "agent" || type === "ai")) ||
      (p.capabilities?.canvasNode as any)?.type === type ||
      p.id.endsWith(`-${type}`) ||
      p.id.includes(type)
  );

  const pluginId = matchedPlugin?.id || type;
  const def = definitions[pluginId] || (matchedPlugin ? definitions[matchedPlugin.id] : undefined);
  const Component = def?.renderCanvasNode || def?.renderNode;

  if (Component) {
    // Plugin views are code-split, so the first mount of a given plugin waits on a
    // chunk fetch. Keep the node's own frame on screen while that happens — the
    // canvas has already laid out a slot for it, and swapping in a differently
    // shaped placeholder would make the node jump.
    return (
      <Suspense fallback={<PluginNodeLoading />}>
        <Component id={id} data={data} selected={selected} />
      </Suspense>
    );
  }

  return (
    <div className="flex h-full w-full flex-col items-center justify-center rounded-lg border border-[var(--border)] bg-[var(--surface)] p-4 font-mono text-xs text-gray-400">
      <span className="text-xl mb-1">🧩</span>
      <span className="font-semibold text-gray-200">Plugin Extension</span>
      <span className="text-[11px] text-gray-500 mt-0.5 text-center">
        Node type <code className="text-cyan-400">{type}</code> is rendered via external plugin.
      </span>
      {matchedPlugin && matchedPlugin.enabled === false && (
        <button
          type="button"
          onClick={() => usePluginStore.getState().togglePlugin(matchedPlugin.id, true)}
          className="mt-2.5 rounded bg-cyan-600 px-3 py-1 text-xs text-white hover:bg-cyan-500"
        >
          Enable {matchedPlugin.name}
        </button>
      )}
    </div>
  );
});

function PluginNodeLoading() {
  return (
    <div className="flex h-full w-full items-center justify-center border border-[var(--border)] bg-[var(--surface)] font-mono text-[11px] text-[var(--text-faint)]">
      Loading…
    </div>
  );
}
