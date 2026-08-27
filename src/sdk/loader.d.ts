import type { PluginDefinition, PluginManifest } from "./plugin";
/**
 * Loads an external compiled plugin bundle dynamically into the browser runtime.
 * Supports file:// paths, blob URLs, and inline ES modules.
 */
export declare function loadPluginBundle(manifest: PluginManifest, bundleSource: string): Promise<PluginDefinition | null>;
//# sourceMappingURL=loader.d.ts.map