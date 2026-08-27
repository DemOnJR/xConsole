import { definePlugin, type PluginDefinition } from "./sdk";
import { AgentNode } from "./AgentNode";
import manifest from "../plugin.json";

export const agentPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderNode: AgentNode,
  apply: () => {
    console.log(`[Plugin Harness] AI Agent Engine plugin mounted`);
    return () => {
      console.log(`[Plugin Harness] AI Agent Engine plugin unmounted`);
    };
  },
});

export default agentPlugin;
