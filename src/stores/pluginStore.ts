import { create } from "zustand";
import { api } from "../lib/tauri";
import {
  type PluginDefinition,
  type PluginManifest,
  type PluginNavItemCapability,
  type PluginAgentToolCapability,
  toCordisPlugin,
} from "../sdk/plugin";
import { rootContext, type Fork } from "../sdk/cordis";
import { loadPluginBundle } from "../sdk/loader";

// Provide Core Services into Cordis Microkernel
rootContext.provide("api", api);

// Track active Cordis forks for temporal composability
const activeForks = new Map<string, Fork>();

export interface FeaturedCommunityPlugin {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  repository: string;
  icon: string;
  category: string;
  stars: number;
  tags: string[];
}

/**
 * Auto-discover all plugins inside the workspace plugins/ directory.
 * No manual imports needed — adding, updating, or removing plugins in plugins/
 * will automatically be detected and registered without modifying core xConsole files!
 */
const discoveredPluginModules = import.meta.glob<{
  default?: PluginDefinition;
  [key: string]: any;
}>("../../plugins/*/src/index.{ts,tsx}", { eager: true });

function getBuiltinPluginDefinitions(): Record<string, PluginDefinition> {
  const defs: Record<string, PluginDefinition> = {};
  for (const [, mod] of Object.entries(discoveredPluginModules)) {
    const candidate =
      mod.default ||
      (mod as any).plugin ||
      Object.values(mod).find(
        (v: any) => v && typeof v === "object" && v.manifest?.id,
      );
    if (candidate && candidate.manifest?.id) {
      defs[candidate.manifest.id] = candidate;
    }
  }
  return defs;
}

import { onPluginInstallProgress, type PluginInstallProgress, type PluginUpdateInfo } from "../lib/tauri";

const COMMUNITY_CATALOG: FeaturedCommunityPlugin[] = [
  {
    id: "xconsole-plugin-redis",
    name: "Redis & Key-Value Inspector",
    version: "0.9.0",
    description: "Inspect Redis keys, real-time memory usage, pub/sub channels, and TTL cache management.",
    author: "Community Devs",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-redis",
    icon: "ZapIcon",
    category: "database",
    stars: 67,
    tags: ["redis", "cache", "memory", "pubsub"],
  },
  {
    id: "xconsole-plugin-docker",
    name: "Docker & Container Orchestrator",
    version: "0.8.5",
    description: "Inspect remote container logs, restart services, monitor CPU/RAM limits, and compose stacks.",
    author: "DevOps Collective",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-docker",
    icon: "ContainerIcon",
    category: "infrastructure",
    stars: 115,
    tags: ["docker", "containers", "logs", "compose"],
  },
  {
    id: "xconsole-plugin-nginx",
    name: "Nginx & SSL Auto-Configurator",
    version: "0.7.0",
    description: "Visual reverse proxy builder, certbot Let's Encrypt renewal, and config syntax tester.",
    author: "WebOps Pro",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-nginx",
    icon: "GlobeIcon",
    category: "networking",
    stars: 53,
    tags: ["nginx", "ssl", "certbot", "reverse-proxy"],
  },
];

export function getFeaturedCommunityPlugins(): FeaturedCommunityPlugin[] {
  const defs = getBuiltinPluginDefinitions();
  const builtins: FeaturedCommunityPlugin[] = Object.values(defs).map((d) => ({
    id: d.manifest.id,
    name: d.manifest.name,
    version: d.manifest.version || "1.0.0",
    description: d.manifest.description || "",
    author: d.manifest.author || "xConsole Team",
    repository: (d.manifest as any).repository || `https://github.com/DemOnJR/${d.manifest.id}`,
    icon: (d.manifest as any).icon || "PuzzleIcon",
    category: (d.manifest as any).category || "extension",
    stars: 120,
    tags: (d.manifest as any).tags || [d.manifest.id],
  }));

  return [...builtins, ...COMMUNITY_CATALOG];
}

export const FEATURED_COMMUNITY_PLUGINS = getFeaturedCommunityPlugins();

export interface InstallProgressState {
  status: "idle" | "installing" | "success" | "error";
  step: string;
  stepIndex: number;
  totalSteps: number;
  percent: number;
  logs: string[];
  error?: string | null;
  pluginName?: string | null;
}

interface PluginState {
  plugins: PluginManifest[];
  definitions: Record<string, PluginDefinition>;
  openViews: Record<string, boolean>;
  activeNavItems: PluginNavItemCapability[];
  activeAgentTools: PluginAgentToolCapability[];
  marketplaceOpen: boolean;
  selectedPluginId: string | null;
  loading: boolean;
  installing: boolean;
  error: string | null;
  installProgress: InstallProgressState | null;

  // Updates & Fork Management
  availableUpdates: Record<string, PluginUpdateInfo>;
  checkingUpdates: boolean;
  updatingPluginIds: Record<string, boolean>;
  autoCheckUpdates: boolean;
  autoUpdateEnabled: boolean;

  // Accessors
  isPluginViewOpen: (pluginId: string) => boolean;

  // Actions
  loadPlugins: () => Promise<void>;
  selectPlugin: (pluginId: string | null) => void;
  getPluginReadme: (pluginId: string) => Promise<string>;
  registerDefinition: (def: PluginDefinition) => void;
  installPlugin: (source: string) => Promise<PluginManifest>;
  linkPlugin: (path: string) => Promise<PluginManifest>;
  uninstallPlugin: (pluginId: string) => Promise<void>;
  togglePlugin: (pluginId: string, enabled?: boolean) => Promise<void>;
  openPluginView: (pluginId: string) => void;
  closePluginView: (pluginId: string) => void;
  togglePluginView: (pluginId: string) => void;
  openMarketplace: () => void;
  closeMarketplace: () => void;
  toggleMarketplace: () => void;
  clearInstallProgress: () => void;

  // Update actions
  checkForUpdates: () => Promise<PluginUpdateInfo[]>;
  checkSinglePluginUpdate: (pluginId: string) => Promise<PluginUpdateInfo>;
  updateSinglePlugin: (pluginId: string) => Promise<PluginManifest>;
  updateAllAvailablePlugins: () => Promise<PluginManifest[]>;
  changePluginRemote: (pluginId: string, newUrl: string) => Promise<string>;
  setAutoCheckUpdates: (enabled: boolean) => void;
  setAutoUpdateEnabled: (enabled: boolean) => void;
}

export const usePluginStore = create<PluginState>((set, get) => ({
  plugins: [],
  definitions: getBuiltinPluginDefinitions(),
  openViews: {},
  activeNavItems: [],
  activeAgentTools: [],
  marketplaceOpen: false,
  selectedPluginId: null,
  loading: false,
  installing: false,
  error: null,
  installProgress: null,
  availableUpdates: {},
  checkingUpdates: false,
  updatingPluginIds: {},
  autoCheckUpdates: localStorage.getItem("xconsole_plugin_auto_check") !== "false",
  autoUpdateEnabled: localStorage.getItem("xconsole_plugin_auto_update") === "true",

  isPluginViewOpen: (pluginId: string) => {
    const norm = pluginId === "analytics" ? "xconsole-plugin-analytics" : pluginId === "cloudflare" ? "xconsole-plugin-cloudflare" : pluginId;
    return Boolean(get().openViews[norm] || get().openViews[pluginId]);
  },

  selectPlugin: (pluginId: string | null) => {
    set({ selectedPluginId: pluginId });
  },

  getPluginReadme: async (pluginId: string) => {
    try {
      return await api.getPluginReadme(pluginId);
    } catch {
      return `# ${pluginId}\n\nDocumentation is available on GitHub: https://github.com/DemOnJR/${pluginId}`;
    }
  },

  loadPlugins: async () => {
    set({ loading: true, error: null });
    try {
      const [backendPlugins, disabledList] = await Promise.all([
        api.listInstalledPlugins().catch(() => []),
        api.getDisabledPluginIds().catch(() => []),
      ]);
      const disabledSet = new Set(disabledList);

      // Merge with discovered and registered definitions
      const defs = { ...getBuiltinPluginDefinitions(), ...get().definitions };
      const mergedMap = new Map<string, PluginManifest>();

      // Put builtin definitions
      for (const [id, def] of Object.entries(defs)) {
        mergedMap.set(id, {
          ...def.manifest,
          enabled: !disabledSet.has(id),
        });
      }

      // Put backend loaded plugins and dynamically load missing definitions
      for (const bp of backendPlugins) {
        const path = bp.installedPath || (bp as any).installed_path;
        const isEnabled = bp.enabled !== false && !disabledSet.has(bp.id);
        if (!defs[bp.id] && path && isEnabled) {
          const bundlePath = `${path}/dist/index.js`;
          try {
            const loaded = await loadPluginBundle(bp, bundlePath);
            if (loaded) {
              defs[bp.id] = loaded;
            }
          } catch {
            // ignore if external bundle is not built yet
          }
        }

        mergedMap.set(bp.id, {
          ...bp,
          enabled: isEnabled,
          capabilities: defs[bp.id]?.manifest?.capabilities ?? bp.capabilities,
        });
      }

      const allPlugins = Array.from(mergedMap.values());

      // Compute active extension slots once
      const activeNavItems = allPlugins
        .filter((p) => {
          const nav = p.capabilities?.navItem || (p.capabilities as any)?.nav_item;
          return p.enabled !== false && Boolean(nav);
        })
        .map((p) => {
          const nav = (p.capabilities?.navItem || (p.capabilities as any)?.nav_item) as any;
          return {
            ...nav,
            id: p.id,
          };
        })
        .sort((a, b) => (a.order ?? 100) - (b.order ?? 100));

      const activeAgentTools: PluginAgentToolCapability[] = [];
      for (const p of allPlugins) {
        const tools = p.capabilities?.agentTools || (p.capabilities as any)?.agent_tools;
        if (p.enabled !== false && tools) {
          activeAgentTools.push(...(tools as any));
        }
      }

      // Spatiotemporal Cordis Reconciliation
      for (const p of allPlugins) {
        const isEnabled = p.enabled !== false;
        const def = defs[p.id];

        if (isEnabled && def && !activeForks.has(p.id)) {
          // Mount fork in Cordis kernel
          const cordisPlugin = toCordisPlugin(def, {
            pluginId: p.id,
            manifest: p,
            api,
            ctx: rootContext,
          });
          const fork = rootContext.plugin(cordisPlugin);
          activeForks.set(p.id, fork);
        } else if (!isEnabled && activeForks.has(p.id)) {
          // Unmount fork and cleanly reverse all effects
          const fork = activeForks.get(p.id);
          if (fork) {
            void fork.dispose();
            activeForks.delete(p.id);
          }
        }
      }

      set({
        plugins: allPlugins,
        definitions: defs,
        activeNavItems,
        activeAgentTools,
        loading: false,
      });

      // Optionally auto-check updates if enabled
      if (get().autoCheckUpdates) {
        get().checkForUpdates().catch(() => {});
      }
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  registerDefinition: (def: PluginDefinition) => {
    set((s) => ({
      definitions: {
        ...s.definitions,
        [def.manifest.id]: def,
      },
    }));
  },

  installPlugin: async (source: string) => {
    set({
      installing: true,
      error: null,
      installProgress: {
        status: "installing",
        step: "Inițializare...",
        stepIndex: 1,
        totalSteps: 5,
        percent: 10,
        logs: [`Pornire instalare plugin din '${source}'...`],
        error: null,
        pluginName: null,
      },
    });

    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onPluginInstallProgress((p: PluginInstallProgress) => {
        set((s) => {
          const prevLogs = s.installProgress?.logs || [];
          const newLogs = p.log_line ? [...prevLogs, p.log_line] : prevLogs;
          return {
            installProgress: {
              status: p.is_error ? "error" : p.is_done ? "success" : "installing",
              step: p.step,
              stepIndex: p.step_index,
              totalSteps: p.total_steps,
              percent: p.percent,
              logs: newLogs,
              error: p.is_error ? (p.log_line || "Instalare eșuată") : null,
              pluginName: s.installProgress?.pluginName || null,
            },
          };
        });
      });

      const manifest = await api.installPlugin(source);
      await get().loadPlugins();
      set((s) => ({
        installing: false,
        installProgress: s.installProgress
          ? {
              ...s.installProgress,
              status: "success",
              step: "Finalizat cu succes!",
              percent: 100,
              pluginName: manifest.name,
            }
          : null,
      }));
      return manifest;
    } catch (e) {
      set((s) => ({
        installing: false,
        error: String(e),
        installProgress: s.installProgress
          ? {
              ...s.installProgress,
              status: "error",
              error: String(e),
            }
          : null,
      }));
      throw e;
    } finally {
      if (unlisten) {
        unlisten();
      }
    }
  },

  clearInstallProgress: () => set({ installProgress: null, error: null }),

  linkPlugin: async (path: string) => {
    set({ installing: true, error: null });
    try {
      const manifest = await api.linkPlugin(path);
      await get().loadPlugins();
      set({ installing: false });
      return manifest;
    } catch (e) {
      set({ installing: false, error: String(e) });
      throw e;
    }
  },

  uninstallPlugin: async (pluginId: string) => {
    try {
      // 1. Unmount and dispose cordis fork
      const fork = activeForks.get(pluginId);
      if (fork) {
        void fork.dispose();
        activeForks.delete(pluginId);
      }

      // 2. Call backend deletion
      await api.uninstallPlugin(pluginId);

      // 3. Remove definition from memory
      set((s) => {
        const nextDefs = { ...s.definitions };
        delete nextDefs[pluginId];
        const nextUpdates = { ...s.availableUpdates };
        delete nextUpdates[pluginId];
        return { definitions: nextDefs, availableUpdates: nextUpdates };
      });

      // 4. Reload plugins list
      await get().loadPlugins();
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  togglePlugin: async (pluginId: string, enabled?: boolean) => {
    const cur = get().plugins.find((p) => p.id === pluginId);
    const targetState = enabled !== undefined ? enabled : !cur?.enabled;
    try {
      await api.togglePlugin(pluginId, targetState);
      await get().loadPlugins();
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  openPluginView: (pluginId: string) => {
    const norm = pluginId === "analytics" ? "xconsole-plugin-analytics" : pluginId === "cloudflare" ? "xconsole-plugin-cloudflare" : pluginId;
    set((s) => ({
      openViews: { ...s.openViews, [norm]: true, [pluginId]: true },
    }));
  },

  closePluginView: (pluginId: string) => {
    const norm = pluginId === "analytics" ? "xconsole-plugin-analytics" : pluginId === "cloudflare" ? "xconsole-plugin-cloudflare" : pluginId;
    set((s) => ({
      openViews: { ...s.openViews, [norm]: false, [pluginId]: false },
    }));
  },

  togglePluginView: (pluginId: string) => {
    const norm = pluginId === "analytics" ? "xconsole-plugin-analytics" : pluginId === "cloudflare" ? "xconsole-plugin-cloudflare" : pluginId;
    set((s) => {
      const nextState = !s.openViews[norm] && !s.openViews[pluginId];
      return {
        openViews: { ...s.openViews, [norm]: nextState, [pluginId]: nextState },
      };
    });
  },

  openMarketplace: () => set({ marketplaceOpen: true }),
  closeMarketplace: () => set({ marketplaceOpen: false }),
  toggleMarketplace: () => set((s) => ({ marketplaceOpen: !s.marketplaceOpen })),

  // Update & Fork methods
  checkForUpdates: async () => {
    set({ checkingUpdates: true });
    try {
      const updates = await api.checkPluginUpdates();
      const updatesMap: Record<string, PluginUpdateInfo> = {};
      for (const u of updates) {
        updatesMap[u.plugin_id] = u;
      }
      set({ availableUpdates: updatesMap, checkingUpdates: false });

      // If auto-update is enabled, silently update pending plugins
      if (get().autoUpdateEnabled) {
        const pending = updates.filter((u) => u.has_update);
        if (pending.length > 0) {
          void get().updateAllAvailablePlugins();
        }
      }

      return updates;
    } catch {
      set({ checkingUpdates: false });
      return [];
    }
  },

  checkSinglePluginUpdate: async (pluginId: string) => {
    try {
      const updateInfo = await api.checkSinglePluginUpdate(pluginId);
      set((s) => ({
        availableUpdates: {
          ...s.availableUpdates,
          [pluginId]: updateInfo,
        },
      }));
      return updateInfo;
    } catch (e) {
      throw e;
    }
  },

  updateSinglePlugin: async (pluginId: string) => {
    set((s) => ({
      updatingPluginIds: { ...s.updatingPluginIds, [pluginId]: true },
      installing: true,
      error: null,
      installProgress: {
        status: "installing",
        step: "Inițializare actualizare...",
        stepIndex: 1,
        totalSteps: 5,
        percent: 10,
        logs: [`Pornire actualizare pentru '${pluginId}'...`],
        error: null,
        pluginName: pluginId,
      },
    }));

    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onPluginInstallProgress((p: PluginInstallProgress) => {
        set((s) => {
          const prevLogs = s.installProgress?.logs || [];
          const newLogs = p.log_line ? [...prevLogs, p.log_line] : prevLogs;
          return {
            installProgress: {
              status: p.is_error ? "error" : p.is_done ? "success" : "installing",
              step: p.step,
              stepIndex: p.step_index,
              totalSteps: p.total_steps,
              percent: p.percent,
              logs: newLogs,
              error: p.is_error ? (p.log_line || "Actualizare eșuată") : null,
              pluginName: s.installProgress?.pluginName || pluginId,
            },
          };
        });
      });

      const manifest = await api.updatePlugin(pluginId);
      await get().loadPlugins();

      // Clear update badge for this plugin
      set((s) => {
        const nextUpdates = { ...s.availableUpdates };
        delete nextUpdates[pluginId];
        const nextUpdating = { ...s.updatingPluginIds };
        delete nextUpdating[pluginId];
        return {
          availableUpdates: nextUpdates,
          updatingPluginIds: nextUpdating,
          installing: false,
          installProgress: s.installProgress
            ? {
                ...s.installProgress,
                status: "success",
                step: "Actualizat cu succes!",
                percent: 100,
                pluginName: manifest.name,
              }
            : null,
        };
      });

      return manifest;
    } catch (e) {
      set((s) => {
        const nextUpdating = { ...s.updatingPluginIds };
        delete nextUpdating[pluginId];
        return {
          updatingPluginIds: nextUpdating,
          installing: false,
          error: String(e),
          installProgress: s.installProgress
            ? {
                ...s.installProgress,
                status: "error",
                error: String(e),
              }
            : null,
        };
      });
      throw e;
    } finally {
      if (unlisten) {
        unlisten();
      }
    }
  },

  updateAllAvailablePlugins: async () => {
    const pendingUpdates = Object.values(get().availableUpdates).filter((u) => u.has_update);
    const updated: PluginManifest[] = [];

    for (const update of pendingUpdates) {
      try {
        const manifest = await get().updateSinglePlugin(update.plugin_id);
        updated.push(manifest);
      } catch (err) {
        console.error(`Eșec la actualizarea pluginului ${update.plugin_id}:`, err);
      }
    }

    return updated;
  },

  changePluginRemote: async (pluginId: string, newUrl: string) => {
    const res = await api.setPluginRemote(pluginId, newUrl);
    await get().checkSinglePluginUpdate(pluginId);
    return res;
  },

  setAutoCheckUpdates: (enabled: boolean) => {
    localStorage.setItem("xconsole_plugin_auto_check", String(enabled));
    set({ autoCheckUpdates: enabled });
  },

  setAutoUpdateEnabled: (enabled: boolean) => {
    localStorage.setItem("xconsole_plugin_auto_update", String(enabled));
    set({ autoUpdateEnabled: enabled });
  },
}));
