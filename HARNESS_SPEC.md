# xConsole Harness & Microkernel Architecture Specification

**Version:** 1.0.0  
**Status:** Canonical Living Specification  
**Scope:** xConsole Core Runtime, Plugin Microkernel, AI Self-Composition & UI Extension Architecture

---

## 1. Executive Summary & Core Philosophy

Modern DevOps, Cloud, and Database tools suffer from **monolithic bloat**: every new cloud provider, database engine, or protocol added directly to an application increases codebase entropy, makes maintenance fragile, and degrades UI performance.

**xConsole** solves this through a pure **Microkernel & Reactive Harness Architecture**. The host application is intentionally minimal (handling only basic windowing, workspace persistence, and VPS SSH connectivity). Every major operational feature—including the **Autonomous AI Agent**, **Remote SFTP File Manager**, **Database Explorer (MySQL/PostgreSQL/SQLite)**, and **Cloudflare Zero Trust**—operates as an independent, hot-swappable plugin with full spatial and temporal composability.

```
+-------------------------------------------------------------------------+
|                              xConsole Host Shell                        |
|   +---------------------+   +---------------------+   +---------------+ |
|   |  Workspaces Engine  |   |  VPS Host Manager   |   | QuickOpen Kbd | |
|   +---------------------+   +---------------------+   +---------------+ |
+------------------------------------+------------------------------------+
                                     |
                +--------------------+--------------------+
                |  xConsole Reactive Microkernel Engine   |
                |  - Service Provider (IoC)               |
                |  - Spatial Reconciliation & Injection   |
                |  - Temporal Fork Teardown (LIFO Stacks) |
                +--------------------+--------------------+
                                     |
     +-----------------+-------------+-------------+-----------------+
     |                 |                           |                 |
+----+----+       +----+----+                 +----+----+       +----+----+
| Cloudflare|     | Database|                 |  SFTP   |     | AI Agent|
|  Plugin |       |  Plugin |                 | Plugin  |     |  Plugin |
+---------+       +---------+                 +---------+     +---------+
```

---

## 2. Core Microkernel Primitives

### 2.1 Context Hierarchy & Service Registry
The foundation is an inverted-control `Context` graph:
- **Root Context (`rootContext`)**: Holds system-level singleton services (e.g. Tauri Rust IPC bridge, OS file handlers, window management).
- **Fork Contexts (`fork`)**: Created whenever a plugin is loaded. A fork can consume dependencies from its ancestor context while isolating its own internal state.

```typescript
export interface Context {
  provide<K extends keyof ContextServices>(name: K, service: ContextServices[K]): void;
  inject<K extends keyof ContextServices>(deps: K[], callback: (services: Pick<ContextServices, K>) => void | Disposable): Disposable;
  plugin(plugin: PluginObject): Fork;
}
```

### 2.2 Revertible Effects & Temporal Composability
When a plugin mounts, it may register navigation icons, canvas nodes, agent tools, or event listeners. To ensure zero resource leaks and instant hot-toggling without restarting the application:
1. Every mounted effect returns a `Disposable` (a cleanup callback).
2. Disposables are stored in a **LIFO (Last-In, First-Out) Teardown Stack**.
3. When a plugin is disabled or uninstalled, `fork.dispose()` drains the stack, reversing all effects in reverse order of mounting.

```typescript
export type Disposable = () => void | Promise<void>;

export interface Fork {
  dispose(): Promise<void>;
  status: "active" | "disposed";
}
```

---

## 3. Spatial Composability & Dynamic Extension Slots

Plugins project capabilities into predefined **Extension Slots** in xConsole:

| Extension Slot | Description | Manifest Key |
| :--- | :--- | :--- |
| **`navItem`** | Injects an icon button into the primary NavRail | `capabilities.navItem` |
| **`canvasNode`** | Injects custom visual nodes onto the ReactFlow infinite canvas | `capabilities.canvasNode` |
| **`agentTools`** | Registers tools callable by the Autonomous AI Agent | `capabilities.agentTools` |
| **`view` / `modal`** | Full-screen or drawer management views (e.g. Cloudflare Manager) | `capabilities.view` |
| **`quickOpenAction`** | Injects fuzzy command palette actions mapped per server | Dynamic derivation |

### 3.1 Reactive Extension Slot Reconciliation
Whenever plugins are installed, updated, or toggled:
- The Zustand `pluginStore` updates `activeNavItems` and `activeAgentTools` atomically.
- NavRail, canvas node factories, and the Quick Open Palette automatically re-render with zero layout shifts or duplicate buttons.

---

## 4. Autonomous AI Agent Self-Composition

A core innovation in xConsole is **Autonomous Agent Tool Self-Composition**:
1. **Dynamic Tool Injection**: The AI Agent's system prompt and tool definitions are not fixed. When a plugin is mounted (e.g., `xconsole-plugin-cloudflare`), its declared tools (`cloudflare_list_tunnels`, `cloudflare_create_dns_record`, etc.) are automatically injected into the LLM runtime.
2. **Autonomous Tool Evolution**: The agent has native access to `plugin_list`, `plugin_install`, and `plugin_toggle`. If asked to execute a MySQL query or configure Nginx, and the respective plugin is missing, the agent can autonomously install the plugin from GitHub, mount it mid-flight, and execute the requested task.

---

## 5. Keyboard-First Fast-Action Routing (Fuzzy Quick Open)

As the plugin ecosystem expands to dozens or hundreds of community extensions, traditional desktop UIs degrade into unusable, cluttered icon grids. 

xConsole implements a **Linux Rofi / Raycast-inspired Fuzzy Command & Plugin Palette**:
- **Hotkeys:** `Ctrl+K`, `Cmd+K`, or `Ctrl+P`.
- **Server Context:** Clicking `⚡ Actions` on any server scopes the palette directly to that machine.
- **Multi-Token Fuzzy Search:** Typing `sftp red` instantly resolves to `SFTP Explorer → RED 0` and opens the node on `Enter`.
- **Dynamic Action Pruning:** If a plugin is disabled, all associated server actions disappear from the palette in sub-millisecond time.

---

## 6. Plugin Manifest Schema (`plugin.json`)

Every community plugin repository must include a root `plugin.json` matching this specification:

```json
{
  "$schema": "https://xconsole.dev/schemas/plugin.json",
  "id": "xconsole-plugin-database",
  "name": "Database & MySQL Explorer",
  "version": "1.0.0",
  "description": "Multi-engine database client with visual schema inspection and SQL runner.",
  "author": "xConsole Team",
  "repository": "https://github.com/DemOnJR/xconsole-plugin-database",
  "icon": "🗄️",
  "category": "database",
  "enabled": true,
  "capabilities": {
    "navItem": {
      "id": "database",
      "label": "Database",
      "icon": "DatabaseIcon",
      "order": 30
    },
    "agentTools": [
      {
        "name": "db_query",
        "description": "Execute a SQL query against a configured database connection.",
        "parameters": {
          "type": "object",
          "properties": {
            "query": { "type": "string", "description": "SQL query to execute" }
          },
          "required": ["query"]
        }
      }
    ]
  }
}
```

---

## 7. Plugin Authoring & Lifecycle Workflow

### 7.1 Defining a Plugin
```typescript
import { definePlugin, type PluginDefinition } from "@xconsole/sdk";
import manifest from "../plugin.json";

export const myPlugin: PluginDefinition = definePlugin({
  manifest,
  apply: (ctx) => {
    // 1. Mount effects / register state
    console.log(`[Plugin] ${manifest.name} mounted.`);

    // 2. Return reversible effect cleanup function
    return () => {
      console.log(`[Plugin] ${manifest.name} unmounted.`);
    };
  }
});

export default myPlugin;
```

### 7.2 1-Command CLI Installation
Community users install plugins via git reference or package name:
```bash
# Install directly from GitHub
xconsole plugin install DemOnJR/xconsole-plugin-database

# Install specific tag or branch
xconsole plugin install https://github.com/DemOnJR/xconsole-plugin-sftp.git
```

---

## 8. Rollback & Audit Safety Protocol

Any destructive action performed by an automated plugin or AI tool must register a reversal snapshot:
- **Cloudflare Plugin**: Maintains audit records with prior DNS targets and tunnel ingress routes. Rollback restores previous configuration via single-click API patch.
- **Database Plugin**: Wraps structural mutations (`DROP TABLE`, `ALTER TABLE`) with schema snapshots.
- **SFTP Plugin**: Preserves previous file versions prior to overwrite in local scratch cache.

---

*This document represents the architectural standard for xConsole and all official/community plugins.*
