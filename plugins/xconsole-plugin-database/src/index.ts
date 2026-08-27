import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import { DatabaseNode } from "../../../src/components/DatabaseNode";
import manifest from "../plugin.json";

export const databasePlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderCanvasNode: DatabaseNode,
  apply: () => {
    console.log(`[Plugin Harness] Database plugin mounted into Cordis context`);
    return () => {
      console.log(`[Plugin Harness] Database plugin unmounted from Cordis context`);
    };
  },
});

export default databasePlugin;
