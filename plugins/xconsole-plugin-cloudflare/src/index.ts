import { lazy } from "react";
import { definePlugin, type PluginDefinition } from "./sdk";
import manifest from "../plugin.json";
// The plugin definition itself is tiny (manifest + lifecycle) and is discovered
// eagerly at startup, but the view is thousands of lines and pulls in its own
// dependencies. Loading it lazily lets Vite emit it as a separate chunk that is
// fetched the first time this plugin is actually opened, instead of adding its
// full weight to the bundle every launch — including for users who never open it.
const CloudflareManager = lazy(() =>
  import("./CloudflareManager").then((m) => ({ default: m.CloudflareManager })),
);

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
