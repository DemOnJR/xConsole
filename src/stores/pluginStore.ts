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
import { cloudflarePlugin } from "../../plugins/xconsole-plugin-cloudflare/src/index";
import { databasePlugin } from "../../plugins/xconsole-plugin-database/src/index";
import { sftpPlugin } from "../../plugins/xconsole-plugin-sftp/src/index";
import { agentPlugin } from "../../plugins/xconsole-plugin-agent/src/index";

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

export const FEATURED_COMMUNITY_PLUGINS: FeaturedCommunityPlugin[] = [
  {
    id: "xconsole-plugin-cloudflare",
    name: "Cloudflare Zero Trust & Security",
    version: "1.0.0",
    description: "Zero Trust Tunnels, Ingress routing, DNS records & WAF protection with instant rollback.",
    author: "xConsole Team",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-cloudflare",
    icon: "☁️",
    category: "infrastructure",
    stars: 142,
    tags: ["cloudflare", "tunnels", "dns", "waf", "security"],
  },
  {
    id: "xconsole-plugin-database",
    name: "Database & MySQL Explorer",
    version: "1.0.0",
    description: "Multi-engine database client with visual table grid, schema viewer, and SQL query runner.",
    author: "xConsole Team",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-database",
    icon: "🗄️",
    category: "database",
    stars: 98,
    tags: ["mysql", "postgres", "sqlite", "sql", "tables"],
  },
  {
    id: "xconsole-plugin-sftp",
    name: "SFTP & Remote File Manager",
    version: "1.0.0",
    description: "Dual-pane remote filesystem explorer, inline code editor, and file permissions over SSH.",
    author: "xConsole Team",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-sftp",
    icon: "📁",
    category: "networking",
    stars: 84,
    tags: ["sftp", "ssh", "files", "transfers"],
  },
  {
    id: "xconsole-plugin-redis",
    name: "Redis & Key-Value Inspector",
    version: "0.9.0",
    description: "Inspect Redis keys, real-time memory usage, pub/sub channels, and TTL cache management.",
    author: "Community Devs",
    repository: "https://github.com/xconsole-plugins/xconsole-plugin-redis",
    icon: "⚡",
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
    icon: "🐳",
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
    icon: "🌐",
    category: "networking",
    stars: 53,
    tags: ["nginx", "ssl", "certbot", "reverse-proxy"],
  },
];

interface PluginState {
  plugins: PluginManifest[];
  definitions: Record<string, PluginDefinition>;
  openViews: Record<string, boolean>;
  marketplaceOpen: boolean;
  loading: boolean;
  installing: boolean;
  error: string | null;

  // Computed / Accessors
  getActiveNavItems: () => PluginNavItemCapability[];
  getActiveAgentTools: () => PluginAgentToolCapability[];
  isPluginViewOpen: (pluginId: string) => boolean;

  // Actions
  loadPlugins: () => Promise<void>;
  registerDefinition: (def: PluginDefinition) => void;
  installPlugin: (source: string) => Promise<PluginManifest>;
  uninstallPlugin: (pluginId: string) => Promise<void>;
  togglePlugin: (pluginId: string, enabled?: boolean) => Promise<void>;
  openPluginView: (pluginId: string) => void;
  closePluginView: (pluginId: string) => void;
  togglePluginView: (pluginId: string) => void;
  openMarketplace: () => void;
  closeMarketplace: () => void;
  toggleMarketplace: () => void;
}

export const usePluginStore = create<PluginState>((set, get) => ({
  plugins: [],
  definitions: {
    "xconsole-plugin-cloudflare": cloudflarePlugin,
    "xconsole-plugin-database": databasePlugin,
    "xconsole-plugin-sftp": sftpPlugin,
    "xconsole-plugin-agent": agentPlugin,
  },
  openViews: {},
  marketplaceOpen: false,
  loading: false,
  installing: false,
  error: null,

  getActiveNavItems: () => {
    const { plugins } = get();
    return plugins
      .filter((p) => p.enabled !== false && p.capabilities?.navItem)
      .map((p) => ({
        ...p.capabilities.navItem!,
        id: p.id,
      }))
      .sort((a, b) => (a.order ?? 100) - (b.order ?? 100));
  },

  getActiveAgentTools: () => {
    const { plugins } = get();
    const tools: PluginAgentToolCapability[] = [];
    for (const p of plugins) {
      if (p.enabled !== false && p.capabilities?.agentTools) {
        tools.push(...p.capabilities.agentTools);
      }
    }
    return tools;
  },

  isPluginViewOpen: (pluginId: string) => {
    return Boolean(get().openViews[pluginId]);
  },

  loadPlugins: async () => {
    set({ loading: true, error: null });
    try {
      const backendPlugins = await api.listInstalledPlugins().catch(() => []);
      
      // Merge with registered definitions
      const defs = get().definitions;
      const mergedMap = new Map<string, PluginManifest>();

      // Put builtin definitions
      for (const [id, def] of Object.entries(defs)) {
        mergedMap.set(id, def.manifest);
      }

      // Put backend loaded plugins
      for (const bp of backendPlugins) {
        mergedMap.set(bp.id, {
          ...bp,
          capabilities: defs[bp.id]?.manifest?.capabilities ?? bp.capabilities,
        });
      }

      const allPlugins = Array.from(mergedMap.values());

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

      set({ plugins: allPlugins, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  registerDefinition: (def: PluginDefinition) => {
    set((state) => ({
      definitions: {
        ...state.definitions,
        [def.manifest.id]: def,
      },
    }));
    void get().loadPlugins();
  },

  installPlugin: async (source: string) => {
    set({ installing: true, error: null });
    try {
      const installed = await api.installPlugin(source);
      await get().loadPlugins();
      set({ installing: false });
      return installed;
    } catch (e) {
      set({ error: String(e), installing: false });
      throw e;
    }
  },

  uninstallPlugin: async (pluginId: string) => {
    try {
      const fork = activeForks.get(pluginId);
      if (fork) {
        await fork.dispose();
        activeForks.delete(pluginId);
      }
      await api.uninstallPlugin(pluginId);
      set((state) => {
        const nextViews = { ...state.openViews };
        delete nextViews[pluginId];
        return { openViews: nextViews };
      });
      await get().loadPlugins();
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  togglePlugin: async (pluginId: string, enabled?: boolean) => {
    const current = get().plugins.find((p) => p.id === pluginId);
    const nextEnabled = enabled !== undefined ? enabled : !(current?.enabled ?? true);
    try {
      if (!nextEnabled) {
        const fork = activeForks.get(pluginId);
        if (fork) {
          await fork.dispose();
          activeForks.delete(pluginId);
        }
        get().closePluginView(pluginId);
      }
      await api.togglePlugin(pluginId, nextEnabled);
      await get().loadPlugins();
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  openPluginView: (pluginId: string) => {
    set((state) => ({
      openViews: { ...state.openViews, [pluginId]: true },
    }));
  },

  closePluginView: (pluginId: string) => {
    set((state) => ({
      openViews: { ...state.openViews, [pluginId]: false },
    }));
  },

  togglePluginView: (pluginId: string) => {
    set((state) => ({
      openViews: {
        ...state.openViews,
        [pluginId]: !state.openViews[pluginId],
      },
    }));
  },

  openMarketplace: () => set({ marketplaceOpen: true }),
  closeMarketplace: () => set({ marketplaceOpen: false }),
  toggleMarketplace: () => set((s) => ({ marketplaceOpen: !s.marketplaceOpen })),
}));
