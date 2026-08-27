import {
  BotIcon,
  DatabaseIcon,
  FolderIcon,
  SettingsIcon,
  TerminalIcon,
} from "./icons";
import { useUiStore } from "../stores/uiStore";
import { useAgentStore } from "../stores/agentStore";
import { useCanvasStore } from "../stores/canvasStore";
import { useVpsStore } from "../stores/vpsStore";
import { useTransferStore } from "../stores/transferStore";
import { usePluginStore } from "../stores/pluginStore";

function RailBtn({
  active,
  title,
  onClick,
  onDoubleClick,
  badge,
  children,
}: {
  active: boolean;
  title: string;
  onClick: () => void;
  onDoubleClick?: () => void;
  badge?: number;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className="xc-icon-btn relative"
      data-active={active ? "true" : "false"}
      data-tooltip={title}
      data-tooltip-side="right"
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      aria-pressed={active}
    >
      {children}
      {badge != null && badge > 0 ? (
        <span className="xc-badge absolute -right-0.5 -top-0.5">{badge > 99 ? "99+" : badge}</span>
      ) : null}
    </button>
  );
}

/** Servers / host list icon (stacked nodes). */
function HostsIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <rect x="3" y="4" width="18" height="6" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
      <rect x="3" y="14" width="18" height="6" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
      <circle cx="7" cy="7" r="1" fill="currentColor" />
      <circle cx="7" cy="17" r="1" fill="currentColor" />
    </svg>
  );
}

/**
 * Compact left icon rail.
 * Toggles drawers without crowding the title bar.
 */
export function NavRail() {
  const leftOpen = useUiStore((s) => s.leftOpen);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const toggleLeft = useUiStore((s) => s.toggleLeft);
  const toggleRight = useUiStore((s) => s.toggleRight);
  const openSettings = useUiStore((s) => s.openSettings);

  const agentNodeId = useCanvasStore((s) =>
    s.nodes.find((n) => n.type === "agent")?.id ?? null,
  );
  const agentOpen = agentNodeId !== null;

  const sftpNodeId = useCanvasStore((s) =>
    s.nodes.find((n) => n.type === "sftp")?.id ?? null,
  );
  const dbNodeId = useCanvasStore((s) =>
    s.nodes.find((n) => n.type === "db")?.id ?? null,
  );

  const pendingApprovals = useAgentStore((s) => s.pendingApprovals.length);
  const pendingQuestions = useAgentStore((s) => s.pendingQuestions.length);
  const hasPlan = useAgentStore((s) => s.pendingPlan !== null);
  const agentBusy = useAgentStore((s) => s.streaming);
  const agentNeedsYou = pendingApprovals + pendingQuestions + (hasPlan ? 1 : 0);

  const activeTransfers = useTransferStore((s) =>
    Object.values(s.jobs).filter((t) => t.state === "running" || t.state === "scanning")
      .length,
  );

  const activeNavItems = usePluginStore((s) => s.activeNavItems);
  const marketplaceOpen = usePluginStore((s) => s.marketplaceOpen);
  const openViews = usePluginStore((s) => s.openViews);

  return (
    <nav className="xc-rail" aria-label="Main navigation">
      <RailBtn
        active={leftOpen}
        title={leftOpen ? "Hide workspaces" : "Workspaces"}
        onClick={toggleLeft}
      >
        <FolderIcon size={18} />
      </RailBtn>

      <RailBtn
        active={rightOpen}
        title={rightOpen ? "Hide servers" : "Servers"}
        onClick={toggleRight}
      >
        <HostsIcon size={18} />
      </RailBtn>

      <div className="my-1 h-px w-6 bg-[var(--border)]" />

      {/* Dynamic Plugin Slots (Cordis Microkernel Extension Points) */}
      {activeNavItems.map((navItem) => {
        const isAgent = navItem.id === "xconsole-plugin-agent" || navItem.id === "agent";
        const isSftp = navItem.id === "xconsole-plugin-sftp" || navItem.id === "sftp";
        const isDb = navItem.id === "xconsole-plugin-database" || navItem.id === "database";
        const isCloudflare = navItem.id === "xconsole-plugin-cloudflare" || navItem.id === "cloudflare";

        if (isAgent) {
          return (
            <RailBtn
              key={navItem.id}
              active={agentOpen}
              title={
                agentNeedsYou > 0
                  ? `Agent needs you (${agentNeedsYou})`
                  : agentBusy
                    ? "Agent working…"
                    : agentOpen
                      ? "Hide agent (double-click fills the canvas)"
                      : "Agent (double-click fills the canvas)"
              }
              onClick={() => useCanvasStore.getState().toggleAgent()}
              onDoubleClick={() => {
                const node = useCanvasStore.getState().nodes.find((n) => n.type === "agent");
                if (node) {
                  useCanvasStore.getState().focus(node.id);
                  useCanvasStore.getState().toggleFillPane(node.id);
                }
              }}
              badge={agentNeedsYou > 0 ? agentNeedsYou : undefined}
            >
              <BotIcon size={18} />
              {agentBusy && agentNeedsYou === 0 ? (
                <span
                  className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-[var(--accent)]"
                  aria-hidden
                />
              ) : null}
            </RailBtn>
          );
        }

        if (isSftp) {
          const sftpActive = Boolean(sftpNodeId);
          return (
            <RailBtn
              key={navItem.id}
              active={sftpActive}
              title={activeTransfers > 0 ? `SFTP (${activeTransfers} active transfers)` : "SFTP & Remote Files"}
              onClick={() => {
                if (sftpNodeId) {
                  useCanvasStore.getState().removeNode(sftpNodeId);
                } else {
                  const srv = useVpsStore.getState().vpsList[0];
                  if (srv) useCanvasStore.getState().addSftp(srv);
                }
              }}
              badge={activeTransfers > 0 ? activeTransfers : undefined}
            >
              <FolderIcon size={18} />
            </RailBtn>
          );
        }

        if (isDb) {
          const dbActive = Boolean(dbNodeId);
          return (
            <RailBtn
              key={navItem.id}
              active={dbActive}
              title="Database & MySQL Explorer"
              onClick={() => {
                if (dbNodeId) {
                  useCanvasStore.getState().removeNode(dbNodeId);
                } else {
                  const srv = useVpsStore.getState().vpsList[0];
                  if (srv) useCanvasStore.getState().addDb(srv);
                }
              }}
            >
              <DatabaseIcon size={18} />
            </RailBtn>
          );
        }

        if (isCloudflare) {
          const isViewOpen = Boolean(openViews[navItem.id] || openViews["xconsole-plugin-cloudflare"]);
          return (
            <RailBtn
              key={navItem.id}
              active={isViewOpen}
              title="Cloudflare (Zero Trust, Tunnels & DNS)"
              onClick={() => usePluginStore.getState().togglePluginView("xconsole-plugin-cloudflare")}
            >
              <CloudIcon size={18} />
            </RailBtn>
          );
        }

        const isAnalytics = navItem.id === "xconsole-plugin-analytics" || navItem.id === "analytics";
        if (isAnalytics) {
          const isViewOpen = Boolean(openViews[navItem.id] || openViews["xconsole-plugin-analytics"]);
          return (
            <RailBtn
              key={navItem.id}
              active={isViewOpen}
              title="Analytics & Resource Telemetry"
              onClick={() => usePluginStore.getState().togglePluginView("xconsole-plugin-analytics")}
            >
              <ChartIcon size={18} />
            </RailBtn>
          );
        }

        // Generic community plugin slot
        const isViewOpen = Boolean(openViews[navItem.id]);
        return (
          <RailBtn
            key={navItem.id}
            active={isViewOpen}
            title={navItem.label}
            onClick={() => usePluginStore.getState().togglePluginView(navItem.id)}
          >
            <span className="text-base leading-none">{navItem.icon || "🧩"}</span>
          </RailBtn>
        );
      })}

      <div className="mt-auto flex flex-col items-center gap-0.5">
        <RailBtn
          active={marketplaceOpen}
          title="Plugin Marketplace & Harness"
          onClick={() => usePluginStore.getState().toggleMarketplace()}
        >
          <span className="text-base leading-none">🧩</span>
        </RailBtn>

        <RailBtn active={false} title="Settings" onClick={() => openSettings()}>
          <SettingsIcon size={18} />
        </RailBtn>
      </div>
    </nav>
  );
}

function ChartIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M4 19V5M4 19h16M8 16v-5M12 16V8M16 16v-8"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
    </svg>
  );
}

function CloudIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M17.5 19H6.5C4.015 19 2 16.985 2 14.5c0-2.222 1.61-4.068 3.75-4.43C6.34 6.87 9.17 4.5 12.5 4.5c3.78 0 6.91 2.94 7.37 6.67 1.76.4 3.13 1.95 3.13 3.83 0 2.21-1.79 4-4 4z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Unused export kept for future focused-host mode icon. */
export function CanvasModeIcon() {
  return <TerminalIcon size={18} />;
}
