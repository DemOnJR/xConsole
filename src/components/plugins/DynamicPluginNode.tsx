import { memo } from "react";
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
      (p.capabilities?.canvasNode as any)?.type === type ||
      p.id === type ||
      p.id.endsWith(`-${type}`),
  );

  const def = matchedPlugin ? definitions[matchedPlugin.id] : definitions[type || ""];
  const Component = def?.renderNode || def?.renderCanvasNode;

  if (Component) {
    return <Component id={id} data={data} selected={selected} />;
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
