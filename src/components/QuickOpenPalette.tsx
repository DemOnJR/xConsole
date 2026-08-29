import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQuickOpenStore } from "../stores/quickOpenStore";
import { useVpsStore } from "../stores/vpsStore";
import { useCanvasStore } from "../stores/canvasStore";
import { usePluginStore } from "../stores/pluginStore";
import { useUiStore } from "../stores/uiStore";
import { useMaskHost } from "../lib/privacy";
import {
  TerminalIcon,
  FolderIcon,
  DatabaseIcon,
  BotIcon,
  CloudIcon,
  PuzzleIcon,
  SettingsIcon,
  ChartIcon,
  GridIcon,
  SearchIcon,
} from "./icons";

interface PaletteItem {
  id: string;
  category: string;
  title: string;
  subtitle?: string;
  icon: ReactNode;
  keywords: string[];
  action: () => void;
}

export function QuickOpenPalette() {
  const isOpen = useQuickOpenStore((s) => s.isOpen);
  const query = useQuickOpenStore((s) => s.query);
  const targetServer = useQuickOpenStore((s) => s.targetServer);
  const close = useQuickOpenStore((s) => s.close);
  const setQuery = useQuickOpenStore((s) => s.setQuery);

  const vpsList = useVpsStore((s) => s.vpsList);
  const plugins = usePluginStore((s) => s.plugins);
  const maskHost = useMaskHost();

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);

  // Check which plugins are active
  const hasSftp = useMemo(
    () => plugins.some((p) => p.id === "xconsole-plugin-sftp" && p.enabled !== false),
    [plugins],
  );
  const hasDatabase = useMemo(
    () => plugins.some((p) => p.id === "xconsole-plugin-database" && p.enabled !== false),
    [plugins],
  );
  const hasAgent = useMemo(
    () => plugins.some((p) => p.id === "xconsole-plugin-agent" && p.enabled !== false),
    [plugins],
  );
  const hasCloudflare = useMemo(
    () => plugins.some((p) => p.id === "xconsole-plugin-cloudflare" && p.enabled !== false),
    [plugins],
  );

  // Global keyboard shortcuts (Ctrl+K, Cmd+K, Ctrl+P)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === "k" || e.key.toLowerCase() === "p")) {
        e.preventDefault();
        e.stopPropagation();
        useQuickOpenStore.getState().toggle();
      }
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, []);

  // Focus input when palette opens
  useEffect(() => {
    if (isOpen) {
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  // Generate complete list of searchable actions
  const allItems = useMemo<PaletteItem[]>(() => {
    const items: PaletteItem[] = [];

    // If targeted at a specific server
    if (targetServer) {
      items.push({
        id: `server-ssh-${targetServer.id}`,
        category: "Server Action",
        title: `SSH Terminal → ${targetServer.name}`,
        subtitle: `${targetServer.username}@${maskHost(targetServer.host)}:${targetServer.port}`,
        icon: <TerminalIcon size={18} />,
        keywords: ["ssh", "terminal", "connect", targetServer.name, targetServer.host, "bash"],
        action: () => {
          useCanvasStore.getState().addVps(targetServer);
          close();
        },
      });

      if (hasSftp) {
        items.push({
          id: `server-sftp-${targetServer.id}`,
          category: "Server Action",
          title: `SFTP Remote Files → ${targetServer.name}`,
          subtitle: `Explore filesystem over SSH (${maskHost(targetServer.host)})`,
          icon: <FolderIcon size={18} />,
          keywords: ["sftp", "ftp", "files", "explorer", "upload", "download", targetServer.name],
          action: () => {
            useCanvasStore.getState().addSftp(targetServer);
            close();
          },
        });
      }

      if (hasDatabase) {
        items.push({
          id: `server-db-${targetServer.id}`,
          category: "Server Action",
          title: `Database & MySQL Client → ${targetServer.name}`,
          subtitle: `Inspect tables and run SQL on ${targetServer.name}`,
          icon: <DatabaseIcon size={18} />,
          keywords: ["database", "mysql", "sql", "postgres", "sqlite", "tables", targetServer.name],
          action: () => {
            useCanvasStore.getState().addDb(targetServer);
            close();
          },
        });
      }

      items.push({
        id: `server-copy-${targetServer.id}`,
        category: "Server Action",
        title: `Copy Connection Info → ${targetServer.name}`,
        subtitle: `${targetServer.username}@${maskHost(targetServer.host)}`,
        icon: <TerminalIcon size={18} />,
        keywords: ["copy", "ip", "host", targetServer.name, targetServer.host],
        action: () => {
          void navigator.clipboard.writeText(`${targetServer.username}@${targetServer.host}`);
          close();
        },
      });

      return items;
    }

    // General Palette Mode (Global servers + plugins)
    for (const srv of vpsList) {
      // 1. SSH Connect
      items.push({
        id: `ssh-${srv.id}`,
        category: "Servers",
        title: `SSH Terminal → ${srv.name}`,
        subtitle: `${srv.username}@${maskHost(srv.host)}:${srv.port}`,
        icon: <TerminalIcon size={18} />,
        keywords: ["ssh", "terminal", "connect", srv.name, srv.host, "shell", "bash"],
        action: () => {
          useCanvasStore.getState().addVps(srv);
          close();
        },
      });

      // 2. SFTP Plugin Action (only if enabled)
      if (hasSftp) {
        items.push({
          id: `sftp-${srv.id}`,
          category: "SFTP Files",
          title: `SFTP Explorer → ${srv.name}`,
          subtitle: `Remote filesystem & editor (${maskHost(srv.host)})`,
          icon: <FolderIcon size={18} />,
          keywords: ["sftp", "ftp", "files", "remote", "editor", srv.name, srv.host],
          action: () => {
            useCanvasStore.getState().addSftp(srv);
            close();
          },
        });
      }

      // 3. Database Plugin Action (only if enabled)
      if (hasDatabase) {
        items.push({
          id: `db-${srv.id}`,
          category: "Databases",
          title: `Database Client → ${srv.name}`,
          subtitle: `MySQL, PostgreSQL, SQLite workspace`,
          icon: <DatabaseIcon size={18} />,
          keywords: ["database", "mysql", "sql", "db", "tables", "postgres", srv.name],
          action: () => {
            useCanvasStore.getState().addDb(srv);
            close();
          },
        });
      }
    }

    // Global Plugins & Capabilities
    if (hasAgent) {
      items.push({
        id: "plugin-agent",
        category: "Plugins",
        title: "Autonomous AI Agent",
        subtitle: "Open AI pairing assistant window with tool execution",
        icon: <BotIcon size={18} />,
        keywords: ["agent", "ai", "chat", "llm", "assistant", "deepseek", "tools"],
        action: () => {
          useCanvasStore.getState().toggleAgent();
          close();
        },
      });
    }

    if (hasCloudflare) {
      items.push({
        id: "plugin-cloudflare",
        category: "Plugins",
        title: "Cloudflare Zero Trust & Tunnels",
        subtitle: "Manage tunnels, ingress DNS records, and WAF rules",
        icon: <CloudIcon size={18} />,
        keywords: ["cloudflare", "cf", "tunnels", "dns", "waf", "security"],
        action: () => {
          usePluginStore.getState().openPluginView("xconsole-plugin-cloudflare");
          close();
        },
      });
    }

    // System Navigation & Tools
    items.push({
      id: "app-marketplace",
      category: "System",
      title: "Plugin Marketplace & Harness",
      subtitle: "Browse, install, and hot-toggle community plugins",
      icon: <PuzzleIcon size={18} />,
      keywords: ["plugins", "marketplace", "store", "harness", "install", "cordis", "extensions"],
      action: () => {
        usePluginStore.getState().openMarketplace();
        close();
      },
    });

    items.push({
      id: "app-settings",
      category: "System",
      title: "Settings & AI Providers",
      subtitle: "Configure API keys, models, themes, and preferences",
      icon: <SettingsIcon size={18} />,
      keywords: ["settings", "preferences", "config", "api", "keys", "providers"],
      action: () => {
        useUiStore.getState().openSettings();
        close();
      },
    });

    items.push({
      id: "app-analytics",
      category: "Plugins",
      title: "Analytics & Telemetry Suite",
      subtitle: "Dashboard, prompt cache rates, tool execution intelligence, CPU/RAM/GPU telemetry",
      icon: <ChartIcon size={18} />,
      keywords: ["analytics", "telemetry", "metrics", "monitoring", "cpu", "ram", "gpu", "charts", "cache", "dashboard"],
      action: () => {
        usePluginStore.getState().togglePluginView("xconsole-plugin-analytics");
        close();
      },
    });

    items.push({
      id: "app-workspaces",
      category: "System",
      title: "Workspaces Panel",
      subtitle: "Browse and switch saved canvas workspaces",
      icon: <GridIcon size={18} />,
      keywords: ["workspace", "drawer", "canvas", "sessions"],
      action: () => {
        useUiStore.getState().toggleLeft();
        close();
      },
    });

    return items;
  }, [targetServer, vpsList, hasSftp, hasDatabase, hasAgent, hasCloudflare, close]);

  // Multi-token fuzzy filtering
  const filteredItems = useMemo(() => {
    const trimmed = query.trim().toLowerCase();
    if (!trimmed) return allItems;

    const tokens = trimmed.split(/\s+/).filter(Boolean);
    return allItems.filter((item) => {
      const searchableText = `${item.title} ${item.subtitle ?? ""} ${item.category} ${item.keywords.join(" ")}`.toLowerCase();
      return tokens.every((token) => searchableText.includes(token));
    });
  }, [allItems, query]);

  // Keyboard navigation inside palette
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev + 1) % Math.max(1, filteredItems.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev - 1 + filteredItems.length) % Math.max(1, filteredItems.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = filteredItems[selectedIndex];
      if (item) {
        item.action();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  // Scroll active item into view
  useEffect(() => {
    if (listRef.current) {
      const activeEl = listRef.current.querySelector(`[data-index="${selectedIndex}"]`);
      if (activeEl) {
        activeEl.scrollIntoView({ block: "nearest" });
      }
    }
  }, [selectedIndex]);

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center bg-black/70 pt-[12vh] backdrop-blur-md animate-in fade-in duration-100"
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div className="w-[min(680px,92vw)] overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--surface-2)] shadow-[0_25px_60px_-15px_rgba(0,0,0,0.7)] flex flex-col animate-in zoom-in-95 duration-150">
        {/* Search Header */}
        <div className="relative flex items-center border-b border-[var(--border)] px-4 py-3.5 bg-[var(--surface)]">
          <div className="flex h-6 w-6 items-center justify-center mr-3 select-none text-[var(--accent)]">
            <SearchIcon size={18} />
          </div>
          <input
            ref={inputRef}
            type="text"
            className="w-full bg-transparent text-base text-[var(--text)] placeholder-[var(--text-dim)] focus:outline-none"
            placeholder={
              targetServer
                ? `Fast action for ${targetServer.name}... (e.g. sftp, db, ssh)`
                : "Type a command, plugin, or server... (e.g. 'sftp red', 'db port', 'agent')"
            }
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(0);
            }}
            onKeyDown={handleKeyDown}
          />
          {query ? (
            <button
              type="button"
              className="rounded px-2 py-1 text-xs text-[var(--text-dim)] hover:bg-[var(--border)]"
              onClick={() => {
                setQuery("");
                inputRef.current?.focus();
              }}
            >
              Clear
            </button>
          ) : (
            <div className="flex items-center gap-1">
              <kbd className="rounded border border-[var(--border)] bg-[var(--surface-2)] px-1.5 py-0.5 text-[10px] font-mono text-[var(--text-dim)]">
                ESC
              </kbd>
            </div>
          )}
        </div>

        {/* Action List */}
        <div ref={listRef} className="max-h-[50vh] overflow-y-auto p-2 divide-y divide-[var(--border)]/20">
          {filteredItems.length === 0 ? (
            <div className="py-12 text-center text-sm text-[var(--text-dim)]">
              No matching actions or plugins found for <span className="text-[var(--text)] font-mono font-medium">"{query}"</span>
            </div>
          ) : (
            filteredItems.map((item, idx) => {
              const isSelected = idx === selectedIndex;
              return (
                <div
                  key={item.id}
                  data-index={idx}
                  className={`flex items-center gap-3.5 rounded-xl px-3.5 py-2.5 cursor-pointer transition-all ${
                    isSelected
                      ? "bg-[var(--accent)] text-white shadow-sm"
                      : "hover:bg-[var(--surface)] text-[var(--text)]"
                  }`}
                  onMouseEnter={() => setSelectedIndex(idx)}
                  onClick={() => item.action()}
                >
                  <div
                    className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-lg ${
                      isSelected
                        ? "bg-white/20 text-white"
                        : "bg-[var(--surface)] text-[var(--accent)] border border-[var(--border)]"
                    }`}
                  >
                    {item.icon}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-sm truncate">{item.title}</span>
                      <span
                        className={`text-[10px] font-mono uppercase tracking-wider px-1.5 py-0.5 rounded ${
                          isSelected ? "bg-white/25 text-white" : "bg-[var(--border)]/60 text-[var(--text-dim)]"
                        }`}
                      >
                        {item.category}
                      </span>
                    </div>
                    {item.subtitle && (
                      <p className={`text-xs truncate ${isSelected ? "text-white/80" : "text-[var(--text-dim)]"}`}>
                        {item.subtitle}
                      </p>
                    )}
                  </div>
                  {isSelected && (
                    <span className="shrink-0 text-xs font-mono text-white/90">
                      ↵ Enter
                    </span>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Linux / Rofi style footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] bg-[var(--surface)] px-4 py-2 text-[11px] text-[var(--text-dim)]">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="font-mono rounded border border-[var(--border)] bg-[var(--surface-2)] px-1 py-0.5 text-[10px]">
                ↑
              </kbd>{" "}
              <kbd className="font-mono rounded border border-[var(--border)] bg-[var(--surface-2)] px-1 py-0.5 text-[10px]">
                ↓
              </kbd>{" "}
              Navigate
            </span>
            <span>
              <kbd className="font-mono rounded border border-[var(--border)] bg-[var(--surface-2)] px-1.5 py-0.5 text-[10px]">
                ↵
              </kbd>{" "}
              Open
            </span>
            <span>
              <kbd className="font-mono rounded border border-[var(--border)] bg-[var(--surface-2)] px-1.5 py-0.5 text-[10px]">
                Esc
              </kbd>{" "}
              Close
            </span>
          </div>
          <span className="font-mono text-[10px]">
            {filteredItems.length} {filteredItems.length === 1 ? "action" : "actions"} available
          </span>
        </div>
      </div>
    </div>
  );
}
