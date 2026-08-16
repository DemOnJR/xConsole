import {
  BotIcon,
  FolderIcon,
  PanelBottomIcon,
  SettingsIcon,
  TerminalIcon,
} from "./icons";
import { useUiStore } from "../stores/uiStore";
import { useAgentStore } from "../stores/agentStore";
import { useCanvasStore } from "../stores/canvasStore";
import { useTransferStore } from "../stores/transferStore";
import { useEditsStore } from "../stores/editsStore";
import { toggleAgentFillPane } from "./agent/AgentNode";

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

/** File transfer / queue icon. */
function TransferIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M7 17V7M7 7l-3 3M7 7l3 3M17 7v10M17 17l-3-3M17 17l3-3"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Agent file-changes / diff icon. */
function DiffIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect x="3" y="1.6" width="10" height="12.8" rx="1.4" stroke="currentColor" strokeWidth="1.2" />
      <path d="M5.4 5.4h2.2M6.5 4.3v2.2" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
      <path d="M5.4 10.5h5.2" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
    </svg>
  );
}

/**
 * Compact left icon rail.
 * Toggles drawers without crowding the title bar.
 */
export function NavRail() {
  const leftOpen = useUiStore((s) => s.leftOpen);
  const mainView = useUiStore((s) => s.mainView);
  const toggleAnalytics = useUiStore((s) => s.toggleAnalytics);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const bottomOpen = useUiStore((s) => s.bottomOpen);
  const toggleLeft = useUiStore((s) => s.toggleLeft);
  const toggleRight = useUiStore((s) => s.toggleRight);
  const toggleBottom = useUiStore((s) => s.toggleBottom);
  const openSettings = useUiStore((s) => s.openSettings);

  const agentNodeId = useCanvasStore((s) =>
    s.nodes.find((n) => n.type === "agent")?.id ?? null,
  );
  const agentOpen = agentNodeId !== null;

  const pendingApprovals = useAgentStore((s) => s.pendingApprovals.length);
  const pendingQuestions = useAgentStore((s) => s.pendingQuestions.length);
  const hasPlan = useAgentStore((s) => s.pendingPlan !== null);
  const agentBusy = useAgentStore((s) => s.streaming);
  const agentNeedsYou = pendingApprovals + pendingQuestions + (hasPlan ? 1 : 0);

  const transfersOpen = useTransferStore((s) => s.open);
  const setTransfersOpen = useTransferStore((s) => s.setOpen);
  const activeTransfers = useTransferStore((s) =>
    Object.values(s.jobs).filter((t) => t.state === "running" || t.state === "scanning")
      .length,
  );

  const changesOpen = useEditsStore((s) => s.open);
  const toggleChanges = useEditsStore((s) => s.toggle);
  const changeCount = useEditsStore((s) => s.changes.length);

  return (
    <nav className="xc-rail" aria-label="Main navigation">
      <RailBtn
        active={leftOpen && mainView === "canvas"}
        title={leftOpen ? "Hide workspaces" : "Workspaces"}
        onClick={toggleLeft}
      >
        <FolderIcon size={18} />
      </RailBtn>

      <RailBtn
        active={mainView === "analytics"}
        title={mainView === "analytics" ? "Back to canvas" : "Analytics"}
        onClick={toggleAnalytics}
      >
        <ChartIcon size={18} />
      </RailBtn>

      <RailBtn
        active={rightOpen}
        title={rightOpen ? "Hide servers" : "Servers"}
        onClick={toggleRight}
      >
        <HostsIcon size={18} />
      </RailBtn>

      <div className="my-1 h-px w-6 bg-[var(--border)]" />

      <RailBtn
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
        onClick={() => useCanvasStore.getState().addAgent()}
        onDoubleClick={() => {
          const node = useCanvasStore.getState().nodes.find((n) => n.type === "agent");
          if (node) {
            useCanvasStore.getState().focus(node.id);
            toggleAgentFillPane(node.id);
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

      <RailBtn
        active={bottomOpen}
        title={bottomOpen ? "Hide console" : "Console"}
        onClick={toggleBottom}
      >
        <PanelBottomIcon size={18} />
      </RailBtn>

      <RailBtn
        active={changesOpen}
        title={changeCount > 0 ? `Changes (${changeCount})` : "Agent changes"}
        onClick={toggleChanges}
        badge={changeCount}
      >
        <DiffIcon size={18} />
      </RailBtn>

      <RailBtn
        active={transfersOpen}
        title={activeTransfers > 0 ? `Transfers (${activeTransfers})` : "Transfers"}
        onClick={() => setTransfersOpen(!transfersOpen)}
        badge={activeTransfers}
      >
        <TransferIcon size={18} />
      </RailBtn>

      <div className="mt-auto flex flex-col items-center gap-0.5">
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

/** Unused export kept for future focused-host mode icon. */
export function CanvasModeIcon() {
  return <TerminalIcon size={18} />;
}
