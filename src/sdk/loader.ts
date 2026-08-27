import type { PluginDefinition, PluginManifest } from "./plugin";

/**
 * Loads an external compiled plugin bundle dynamically into the browser runtime.
 * Supports file:// paths, blob URLs, and inline ES modules.
 */
export async function loadPluginBundle(
  manifest: PluginManifest,
  bundleSource: string,
): Promise<PluginDefinition | null> {
  try {
    let moduleUrl = bundleSource;

    // If source is raw JavaScript code (not a URL/path)
    if (bundleSource.includes("export ") || bundleSource.includes("definePlugin")) {
      const blob = new Blob([bundleSource], { type: "application/javascript" });
      moduleUrl = URL.createObjectURL(blob);
    }

    // Dynamic import with bundler hint
    const imported = await import(/* @vite-ignore */ moduleUrl);
    const pluginDef: PluginDefinition = imported.default || imported.plugin || imported;

    if (!pluginDef || typeof pluginDef !== "object") {
      throw new Error(`Plugin bundle for '${manifest.id}' does not export a valid PluginDefinition.`);
    }

    return {
      ...pluginDef,
      manifest: {
        ...manifest,
        ...pluginDef.manifest,
      },
    };
  } catch (err) {
    console.error(`[Plugin Loader] Failed to load bundle for '${manifest.id}':`, err);
    return null;
  }
}
