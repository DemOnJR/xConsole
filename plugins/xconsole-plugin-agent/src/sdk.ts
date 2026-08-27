import type React from "react";

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  icon: string;
  category: string;
  capabilities?: Record<string, any>;
}

export interface PluginDefinition {
  manifest: PluginManifest;
  renderView?: React.ComponentType<{ onClose?: () => void }>;
  renderNode?: React.ComponentType<any>;
  renderCanvasNode?: React.ComponentType<any>;
  apply?: (ctx?: any) => void | (() => void) | Promise<void | (() => void)>;
}

export function definePlugin(def: PluginDefinition): PluginDefinition {
  return def;
}
