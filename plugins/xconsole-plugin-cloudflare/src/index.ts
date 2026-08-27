import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import { CloudflareManager } from "./CloudflareManager";
import manifest from "../plugin.json";

export const cloudflarePlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderView: CloudflareManager,
  apply: () => {
    console.log(`[Plugin Harness] Cloudflare plugin mounted`);
    return () => {
      console.log(`[Plugin Harness] Cloudflare plugin unmounted`);
    };
  },
});

export default cloudflarePlugin;
