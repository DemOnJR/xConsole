import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/tauri";
import { SettingsIcon } from "./icons";
import { useUiStore } from "../stores/uiStore";
import { Toolbar } from "./Toolbar";

const appWindow = getCurrentWindow();

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
        onClick={() => {
          // Recorded so the log can distinguish a deliberate close from a WM_CLOSE
          // arriving from outside the app -- they are indistinguishable in the Rust
          // event loop, and that ambiguity is the whole question here.
          void api.logDiag("title-bar close button clicked");
          void appWindow.close();
        }}
      >
        <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden>
          <path d="M1 1 L10 10 M10 1 L1 10" stroke="currentColor" strokeWidth="1.1" />
        </svg>
      </button>
    </div>
  );
}

export function AppToolbar() {
  const openSettings = useUiStore((s) => s.openSettings);

  return (
    <header
      data-tauri-drag-region
      className="flex h-[var(--titlebar-h)] shrink-0 items-center border-b border-[var(--border)] bg-[var(--surface-2)] pl-2"
    >
      {/* Brand mark — panel toggles live on the left nav rail. */}
      <div className="flex items-center gap-2 pr-2">
        <span className="select-none text-xs font-semibold tracking-wide text-[var(--text-dim)]">
          xConsole
        </span>
        <button
          type="button"
          className="xc-icon-btn"
          data-tooltip="Settings"
          data-tooltip-side="bottom"
          onClick={() => openSettings()}
        >
          <SettingsIcon size={15} />
        </button>
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
