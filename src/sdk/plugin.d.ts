import React from "react";
import { type api } from "../lib/tauri";
import { Context, type Disposable, type PluginObject } from "./cordis";
export type PluginCategory = "infrastructure" | "database" | "networking" | "ai" | "developer" | "security" | "monitoring" | "other";
export interface PluginNavItemCapability {
    id: string;
    label: string;
    icon: string;
    order?: number;
    badge?: string;
}
export interface PluginAgentToolCapability {
    name: string;
    description: string;
    parameters: Record<string, unknown>;
}
export interface PluginSettingsCapability {
    id: string;
    title: string;
    icon?: string;
}
export interface PluginCanvasNodeCapability {
    type: string;
    title: string;
    defaultWidth?: number;
    defaultHeight?: number;
}
export interface PluginCommandCapability {
    id: string;
    title: string;
    shortcut?: string;
}
export interface PluginCapabilities {
    navItem?: PluginNavItemCapability;
    agentTools?: PluginAgentToolCapability[];
    settingsSection?: PluginSettingsCapability;
    canvasNode?: PluginCanvasNodeCapability;
    commands?: PluginCommandCapability[];
}
export interface PluginManifest {
    id: string;
    name: string;
    version: string;
    description: string;
    author: string;
    homepage?: string;
    repository?: string;
    icon: string;
    category: PluginCategory | string;
    enabled?: boolean;
    isBuiltin?: boolean;
    installedPath?: string;
    using?: string[];
    capabilities?: PluginCapabilities | Record<string, any>;
}
export interface PluginContext {
    pluginId: string;
    manifest: PluginManifest;
    api: typeof api;
    ctx: Context;
}
export interface PluginDefinition {
    manifest: PluginManifest;
    using?: string[];
    renderView?: React.ComponentType<{
        onClose?: () => void;
    }>;
    renderSettings?: React.ComponentType;
    renderCanvasNode?: React.ComponentType<any>;
    renderNode?: React.ComponentType<any>;
    apply?: (ctx: Context, pluginCtx: PluginContext) => Disposable | void | Promise<Disposable | void>;
    onMount?: (ctx: PluginContext) => Promise<void> | void;
    onUnmount?: (ctx: PluginContext) => Promise<void> | void;
}
/** Helper to define a typed xConsole plugin with intellisense */
export declare function definePlugin(definition: PluginDefinition): PluginDefinition;
/** Convert a PluginDefinition into a Cordis PluginObject */
export declare function toCordisPlugin(definition: PluginDefinition, pluginCtx: PluginContext): PluginObject;
//# sourceMappingURL=plugin.d.ts.map