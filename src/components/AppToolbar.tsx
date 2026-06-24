import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  BotIcon,
  PanelBottomIcon,
  PanelLeftIcon,
  PanelRightIcon,
  SettingsIcon,
} from "./icons";
import { useUiStore } from "../stores/uiStore";
import { useEditsStore } from "../stores/editsStore";
import { Toolbar } from "./Toolbar";

const appWindow = getCurrentWindow();

/** Document with a "+" and "−" — the agent's file changes / diff view. */
function DiffIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden>
      <rect x="3" y="1.6" width="10" height="12.8" rx="1.4" stroke="currentColor" strokeWidth="1.2" />
      <path d="M5.4 5.4h2.2M6.5 4.3v2.2" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
      <path d="M5.4 10.5h5.2" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
    </svg>
  );
}

function ToggleBtn({
  active,
  title,
  onClick,
  children,
}: {
  active: boolean;
  title: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      data-tooltip={title}
      data-tooltip-side="bottom"
      onClick={onClick}
      className={`rounded-md p-1.5 transition ${
        active
          ? "bg-[var(--border)] text-gray-100"
          : "text-gray-500 hover:bg-[var(--surface)] hover:text-gray-300"
      }`}
    >
      {children}
    </button>
  );
}

/** Frameless-window caption controls (right side). */
function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    appWindow.isMaximized().then(setMaximized).catch(() => {});
    appWindow
      .onResized(() => {
        appWindow.isMaximized().then(setMaximized).catch(() => {});
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const btn =
    "flex h-9 w-11 items-center justify-center text-gray-400 transition hover:bg-[var(--border)] hover:text-gray-100";

  return (
    <div className="flex items-center">
      <button className={btn} data-tooltip="Minimize" data-tooltip-side="bottom" onClick={() => appWindow.minimize()}>
        <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden>
          <rect x="1" y="5" width="9" height="1" fill="currentColor" />
        </svg>
      </button>
      <button
        className={btn}
        data-tooltip={maximized ? "Restore" : "Maximize"}
        data-tooltip-side="bottom"
        onClick={() => appWindow.toggleMaximize()}
      >
        {maximized ? (
          <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden>
            <rect x="2.5" y="0.5" width="7" height="7" fill="none" stroke="currentColor" />
            <rect x="0.5" y="2.5" width="7" height="7" fill="var(--surface-2)" stroke="currentColor" />
          </svg>
        ) : (
          <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden>
            <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
          </svg>
        )}
      </button>
      <button
        className="flex h-9 w-11 items-center justify-center text-gray-400 transition hover:bg-red-600 hover:text-white"
        data-tooltip="Close"
        data-tooltip-side="bottom"
        onClick={() => appWindow.close()}
      >
        <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden>
          <path d="M1 1 L10 10 M10 1 L1 10" stroke="currentColor" strokeWidth="1.1" />
        </svg>
      </button>
    </div>
  );
}

export function AppToolbar() {
  const leftOpen = useUiStore((s) => s.leftOpen);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const bottomOpen = useUiStore((s) => s.bottomOpen);
  const agentOpen = useUiStore((s) => s.agentOpen);
  const toggleLeft = useUiStore((s) => s.toggleLeft);
  const toggleRight = useUiStore((s) => s.toggleRight);
  const toggleBottom = useUiStore((s) => s.toggleBottom);
  const toggleAgent = useUiStore((s) => s.toggleAgent);
  const openSettings = useUiStore((s) => s.openSettings);
  const changesOpen = useEditsStore((s) => s.open);
  const toggleChanges = useEditsStore((s) => s.toggle);
  const changeCount = useEditsStore((s) => s.changes.length);

  return (
    <header
      data-tauri-drag-region
      className="flex h-9 shrink-0 items-center border-b border-[var(--border)] bg-[var(--surface-2)] pl-2"
    >
      {/* Left: panel toggles + settings */}
      <div className="flex items-center gap-0.5">
        <ToggleBtn
          active={leftOpen}
          title={leftOpen ? "Hide workspaces" : "Show workspaces"}
          onClick={toggleLeft}
        >
          <PanelLeftIcon size={16} />
        </ToggleBtn>
        <ToggleBtn
          active={bottomOpen}
          title={bottomOpen ? "Hide console" : "Show console"}
          onClick={toggleBottom}
        >
          <PanelBottomIcon size={16} />
        </ToggleBtn>
        <ToggleBtn
          active={agentOpen}
          title={agentOpen ? "Hide agent" : "Show agent"}
          onClick={toggleAgent}
        >
          <BotIcon size={16} />
        </ToggleBtn>
        <ToggleBtn
          active={rightOpen}
          title={rightOpen ? "Hide servers" : "Show servers"}
          onClick={toggleRight}
        >
          <PanelRightIcon size={16} />
        </ToggleBtn>

        <div className="mx-1 h-4 w-px bg-[var(--border)]" />

        <div className="relative">
          <ToggleBtn
            active={changesOpen}
            title={changeCount > 0 ? `Changes (${changeCount})` : "Changes the agent made"}
            onClick={toggleChanges}
          >
            <DiffIcon size={16} />
          </ToggleBtn>
          {changeCount > 0 && (
            <span className="pointer-events-none absolute -right-0.5 -top-0.5 flex h-3.5 min-w-[14px] items-center justify-center rounded-full bg-blue-600 px-1 text-[9px] font-semibold leading-none text-white">
              {changeCount}
            </span>
          )}
        </div>

        <ToggleBtn active={false} title="Settings" onClick={() => openSettings()}>
          <SettingsIcon size={16} />
        </ToggleBtn>
      </div>

      {/* Middle: canvas/workspace toolbar, centered. The flanking spacers are the
          window-drag regions; the toolbar itself stays clickable. */}
      <div data-tauri-drag-region className="h-full min-w-4 flex-1" />
      <Toolbar />
      <div data-tauri-drag-region className="h-full min-w-4 flex-1" />

      {/* Right: window caption controls */}
      <WindowControls />
    </header>
  );
}
