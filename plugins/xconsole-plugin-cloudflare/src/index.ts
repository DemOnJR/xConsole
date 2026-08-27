import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import { CloudflareManager } from "../../../src/components/cloudflare/CloudflareManager";
import manifest from "../plugin.json";

export const cloudflarePlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderView: CloudflareManager,
  onMount: async ({ pluginId }) => {
    console.log(`[Plugin Harness] Mounted plugin: ${pluginId}`);
  },
  onUnmount: async ({ pluginId }) => {
    console.log(`[Plugin Harness] Unmounted plugin: ${pluginId}`);
  },
});

export default cloudflarePlugin;
