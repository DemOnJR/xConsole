import { definePlugin, type PluginDefinition } from "./sdk";
import { DatabaseNode } from "./DatabaseNode";
import manifest from "../plugin.json";

export const databasePlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderNode: DatabaseNode,
  renderCanvasNode: DatabaseNode,
  apply: () => {
    console.log(`[Plugin Harness] Database MySQL plugin mounted`);
    return () => {
      console.log(`[Plugin Harness] Database MySQL plugin unmounted`);
    };
  },
});

export default databasePlugin;

