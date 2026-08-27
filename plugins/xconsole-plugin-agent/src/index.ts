import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import { AgentNodeView } from "../../../src/components/agent/AgentNode";
import manifest from "../plugin.json";

export const agentPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderCanvasNode: AgentNodeView,
  apply: () => {
    console.log(`[Plugin Harness] AI Agent Engine plugin mounted into Cordis context`);
    return () => {
      console.log(`[Plugin Harness] AI Agent Engine plugin unmounted from Cordis context`);
    };
  },
});

export default agentPlugin;
