import { definePlugin, type PluginDefinition } from "./sdk";
import { AgentNodeView } from "./AgentNode";
import manifest from "../plugin.json";

export const agentPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderNode: AgentNodeView,
  renderCanvasNode: AgentNodeView,
  apply: () => {
    console.log(`[Plugin Harness] Autonomous AI Agent Engine mounted`);
    return () => {
      console.log(`[Plugin Harness] Autonomous AI Agent Engine unmounted`);
    };
  },
});

export default agentPlugin;
export { AgentNodeView };
