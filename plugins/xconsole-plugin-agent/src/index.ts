import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import manifest from "../plugin.json";

export const agentPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  apply: () => {
    console.log(`[Plugin Harness] AI Agent Engine plugin mounted`);
    return () => {
      console.log(`[Plugin Harness] AI Agent Engine plugin unmounted`);
    };
  },
});

export default agentPlugin;
