import { useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  b64ToBytes,
  onSessionOutput,
  onSessionStatus,
  strToB64,
} from "../lib/tauri";
import { useVpsStore } from "../stores/vpsStore";
import { useWorkspaceStore, parseSavedNodes } from "../stores/workspaceStore";
import { useCanvasStore } from "../stores/canvasStore";
import { useUiStore } from "../stores/uiStore";
import type { ConnState } from "../stores/sessionStore";
import { useXtermScaleFix } from "../hooks/useXtermScaleFix";
import { useThemeStore } from "../stores/themeStore";
import {
  ChevronDownIcon,
  ChevronUpIcon,
  PlugIcon,
  TerminalIcon,
} from "./icons";
const STATUS_COLOR: Record<ConnState, string> = {
  connecting: "#e0af68",
  connected: "#9ece6a",
  reconnecting: "#e0af68",
  disconnected: "#6b7280",
  error: "#f7768e",
};

const CANVAS_CTX = "__canvas__";
const ALL_CTX = "__all__";
const PANEL_H = 320;

interface Target {
  vpsId: string;
  name: string;
  host: string;
}

/**
 * Console SSH sessions, keyed by vpsId, kept alive across pane unmounts so that
 * toggling a target off/on (or reopening the drawer) reattaches instead of
 * reconnecting. These are independent from the canvas sessions.
 */
const consoleSessions = new Map<string, string>();

/** A single server's live terminal inside the console drawer. */
function ConsolePane({
  target,
  broadcastRef,
  broadcastInput,
  onClose,
  onRegistryChange,
  visible,
}: {
  target: Target;
  broadcastRef: React.RefObject<boolean>;
  broadcastInput: (b64: string) => void;
  onClose: (vpsId: string) => void;
  onRegistryChange: () => void;
  visible: boolean;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const [status, setStatus] = useState<ConnState>("connecting");
  const themeId = useThemeStore((s) => s.themeId);
  const customVars = useThemeStore((s) => s.customVars);

  // The console drawer is never under a canvas zoom transform, so it's always 1:1
  // (the hook is a no-op at scale 1, but kept consistent for future-proofing).
  const scaleRef = useRef(1);
  useXtermScaleFix(termRef, scaleRef);

  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = useThemeStore.getState().xterm();
  }, [themeId, customVars]);

  useEffect(() => {
    let mounted = true;
    const unlisteners: UnlistenFn[] = [];

    const term = new Terminal({
      fontFamily:
        '"Cascadia Code", "JetBrains Mono", "Fira Code", Consolas, monospace',
      fontSize: 12,
      cursorBlink: true,
      scrollback: 8000,
      theme: useThemeStore.getState().xterm(),
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    termRef.current = term;
    fitRef.current = fit;
    if (containerRef.current) term.open(containerRef.current);
    try {
      fit.fit();
    } catch {
      /* not measurable yet */
    }

    const statusMap: Record<string, ConnState> = {
      Connecting: "connecting",
      Connected: "connected",
      Reconnecting: "reconnecting",
      Disconnected: "disconnected",
      Error: "error",
    };

    const attach = async (sid: string) => {
      sessionIdRef.current = sid;
      consoleSessions.set(target.vpsId, sid);
      onRegistryChange();
      unlisteners.push(
        await onSessionStatus(
          sid,
          (st) => mounted && setStatus(statusMap[st.kind] ?? "disconnected"),
        ),
      );
      unlisteners.push(
        await onSessionOutput(sid, (bytes) => term.write(bytes)),
      );
      const replay = await api.sshReplay(sid);
      if (replay) term.write(b64ToBytes(replay));
    };

    (async () => {
      try {
        const existing = consoleSessions.get(target.vpsId);
        if (existing) {
          const r = await api.sshReplay(existing).catch(() => null);
          if (!mounted) return;
          if (r !== null) {
            setStatus("connected");
            await attach(existing);
            return;
          }
          consoleSessions.delete(target.vpsId);
        }
        setStatus("connecting");
        const outcome = await api.sshConnect(
          target.vpsId,
          term.cols || 120,
          term.rows || 30,
        );
        if (!mounted) {
          await api.sshDisconnect(outcome.session_id);
          return;
        }
        setStatus("connected");
        await attach(outcome.session_id);
      } catch (e) {
        if (mounted) {
          setStatus("error");
          term.writeln(`\r\n\x1b[31mConnection failed: ${String(e)}\x1b[0m`);
        }
      }
    })();

    const dataSub = term.onData((d) => {
      const b64 = strToB64(d);
      if (broadcastRef.current) {
        broadcastInput(b64);
      } else {
        const sid = sessionIdRef.current;
        if (sid) api.sshWrite(sid, b64).catch(() => {});
      }
    });

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
        const sid = sessionIdRef.current;
        if (sid) api.sshResize(sid, term.cols, term.rows).catch(() => {});
      } catch {
        /* ignore */
      }
    });
    if (containerRef.current) ro.observe(containerRef.current);

    return () => {
      // Keep the backend session alive (registry) so reselecting reattaches.
      mounted = false;
      dataSub.dispose();
      ro.disconnect();
      unlisteners.forEach((u) => u());
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target.vpsId]);

  useEffect(() => {
    if (!visible) return;
    requestAnimationFrame(() => {
      try {
        fitRef.current?.fit();
        const sid = sessionIdRef.current;
        const term = termRef.current;
        if (sid && term) api.sshResize(sid, term.cols, term.rows).catch(() => {});
      } catch {
        /* ignore */
      }
    });
  }, [visible]);

  return (
    <div className="flex min-w-[260px] flex-1 flex-col overflow-hidden rounded-md border border-[var(--border)]">
      <div className="flex items-center gap-1.5 border-b border-[var(--border)] bg-[var(--surface)] px-2 py-1 text-[11px]">
        <span
          className="inline-block h-2 w-2 rounded-full"
          style={{ background: STATUS_COLOR[status] }}
          data-tooltip={status}
        />
        <span className="truncate font-medium text-gray-200">{target.name}</span>
        <span className="truncate text-gray-500">{target.host}</span>
        <button
          className="ml-auto rounded px-1 text-gray-500 hover:bg-[var(--border)] hover:text-gray-200"
          data-tooltip="Close this terminal"
          onClick={() => onClose(target.vpsId)}
        >
          ✕
        </button>
      </div>
      <div
        ref={containerRef}
        className="xterm-host min-h-0 flex-1"
        onClick={() => termRef.current?.focus()}
      />
    </div>
  );
}

export function BottomBar() {
  const vpsList = useVpsStore((s) => s.vpsList);
  const loadVps = useVpsStore((s) => s.load);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const activeId = useWorkspaceStore((s) => s.activeId);
  const canvasNodes = useCanvasStore((s) => s.nodes);

  const [contextId, setContextId] = useState<string>(CANVAS_CTX);
  const [allMode, setAllMode] = useState(true);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const open = useUiStore((s) => s.consoleExpanded);
  const toggleConsoleExpanded = useUiStore((s) => s.toggleConsoleExpanded);
  const broadcast = useUiStore((s) => s.consoleBroadcast);
  const toggleConsoleBroadcast = useUiStore((s) => s.toggleConsoleBroadcast);
  const [activeCount, setActiveCount] = useState(0);

  const broadcastRef = useRef(broadcast);
  useEffect(() => {
    broadcastRef.current = broadcast;
  }, [broadcast]);

  useEffect(() => {
    loadVps();
  }, [loadVps]);

  const targets: Target[] = useMemo(() => {
    if (contextId === CANVAS_CTX) {
      const seen = new Set<string>();
      const out: Target[] = [];
      for (const n of canvasNodes) {
        if (seen.has(n.data.vpsId)) continue;
        seen.add(n.data.vpsId);
        out.push({ vpsId: n.data.vpsId, name: n.data.name, host: n.data.host });
      }
      return out;
    }
    let ids: string[];
    if (contextId === ALL_CTX) {
      ids = vpsList.map((v) => v.id);
    } else {
      const ws = workspaces.find((w) => w.id === contextId);
      const saved = parseSavedNodes(ws?.nodes_json).nodes;
      ids = Array.from(new Set(saved.map((s) => s.vpsId)));
    }
    return ids
      .map((id) => {
        const v = vpsList.find((x) => x.id === id);
        return v ? { vpsId: id, name: v.name, host: v.host } : null;
      })
      .filter((t): t is Target => t !== null);
  }, [contextId, canvasNodes, workspaces, vpsList]);

  useEffect(() => {
    setAllMode(true);
    setSelected(new Set());
  }, [contextId]);

  const effectiveTargets = useMemo(
    () => (allMode ? targets : targets.filter((t) => selected.has(t.vpsId))),
    [allMode, selected, targets],
  );

  const effectiveTargetsRef = useRef<Target[]>([]);
  useEffect(() => {
    effectiveTargetsRef.current = effectiveTargets;
  }, [effectiveTargets]);

  const broadcastInput = (b64: string) => {
    effectiveTargetsRef.current.forEach((t) => {
      const sid = consoleSessions.get(t.vpsId);
      if (sid) api.sshWrite(sid, b64).catch(() => {});
    });
  };

  const toggleTarget = (vpsId: string) => {
    setSelected((prev) => {
      const next = new Set(allMode ? targets.map((t) => t.vpsId) : prev);
      if (next.has(vpsId)) next.delete(vpsId);
      else next.add(vpsId);
      return next;
    });
    setAllMode(false);
  };

  const closePane = (vpsId: string) => {
    const sid = consoleSessions.get(vpsId);
    if (sid) api.sshDisconnect(sid).catch(() => {});
    consoleSessions.delete(vpsId);
    setActiveCount(consoleSessions.size);
    // Drop it from the visible target set.
    setSelected(() => {
      const next = new Set(
        (allMode ? targets : effectiveTargets).map((t) => t.vpsId),
      );
      next.delete(vpsId);
      return next;
    });
    setAllMode(false);
  };

  const disconnectAll = () => {
    consoleSessions.forEach((sid) => api.sshDisconnect(sid).catch(() => {}));
    consoleSessions.clear();
    setActiveCount(0);
    setAllMode(false);
    setSelected(new Set());
  };

  const contextLabel =
    contextId === CANVAS_CTX
      ? "Canvas"
      : contextId === ALL_CTX
        ? "All servers"
        : workspaces.find((w) => w.id === contextId)?.name ?? "Workspace";

  return (
    <div
      className="flex shrink-0 flex-col border-t border-[var(--border)] bg-[var(--surface-2)]"
      style={{ height: open ? PANEL_H : 36 }}
    >
      <div className="flex items-center gap-2 px-2 py-1.5">
        <button
          className="flex items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-gray-200 hover:bg-[var(--border)]"
          data-tooltip={open ? "Collapse console height" : "Expand console height"}
          onClick={() => toggleConsoleExpanded()}
        >
          <TerminalIcon size={15} />
          <span className="font-medium">Console</span>
          {open ? <ChevronDownIcon size={14} /> : <ChevronUpIcon size={14} />}
        </button>

        <div className="mx-0.5 h-5 w-px bg-[var(--border)]" />

        <select
          value={contextId}
          onChange={(e) => setContextId(e.target.value)}
          data-tooltip="Which servers this console can target"
          className="rounded-md border border-[var(--border)] bg-[var(--bg)] px-2 py-1 text-xs text-gray-200 outline-none focus:border-blue-500"
        >
          <option value={CANVAS_CTX}>
            Canvas{activeId ? " (current)" : ""}
          </option>
          <option value={ALL_CTX}>All servers</option>
          <optgroup label="Workspaces">
            {workspaces.map((w) => (
              <option key={w.id} value={w.id}>
                {w.icon ? `${w.icon} ` : ""}
                {w.name}
              </option>
            ))}
          </optgroup>
        </select>

        <button
          onClick={() => {
            setAllMode(true);
            setSelected(new Set());
          }}
          className={`rounded-md border px-2 py-1 text-xs ${
            allMode
              ? "border-blue-500 bg-blue-600 text-white"
              : "border-[var(--border)] text-gray-300 hover:bg-[var(--border)]"
          }`}
          data-tooltip="Open a terminal for every server in this context"
        >
          All ({targets.length})
        </button>

        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
          {targets.map((t) => {
            const on = effectiveTargets.some((e) => e.vpsId === t.vpsId);
            return (
              <button
                key={t.vpsId}
                onClick={() => toggleTarget(t.vpsId)}
                data-tooltip={`${t.name} (${t.host})`}
                className={`shrink-0 rounded-full border px-2 py-0.5 text-[11px] ${
                  on
                    ? "border-blue-500 bg-blue-600/30 text-blue-100"
                    : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)]"
                }`}
              >
                {t.name}
              </button>
            );
          })}
          {targets.length === 0 && (
            <span className="text-[11px] text-gray-600">
              No servers in {contextLabel}.
            </span>
          )}
        </div>

        {/* Broadcast input toggle (only matters with >1 pane) */}
        <button
          onClick={() => toggleConsoleBroadcast()}
          disabled={effectiveTargets.length <= 1}
          className={`rounded-md border px-2 py-1 text-[11px] ${
            broadcast && effectiveTargets.length > 1
              ? "border-amber-500 bg-amber-600/30 text-amber-200"
              : "border-[var(--border)] text-gray-400 hover:bg-[var(--border)] disabled:opacity-40"
          }`}
          data-tooltip="When on, keystrokes are sent to every open terminal at once"
        >
          Broadcast {broadcast ? "on" : "off"}
        </button>

        {activeCount > 0 && (
          <button
            onClick={disconnectAll}
            className="flex items-center gap-1 rounded-md border border-[var(--border)] px-1.5 py-1 text-[11px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            data-tooltip="Close all console SSH sessions"
          >
            <PlugIcon size={13} />
            {activeCount}
          </button>
        )}
      </div>

      {/* Split terminal panes — one per target */}
      <div className={open ? "min-h-0 flex-1" : "hidden"}>
        {effectiveTargets.length === 0 ? (
          <div className="flex h-full items-center justify-center text-xs text-gray-600">
            Select one or more servers above to open terminals.
          </div>
        ) : (
          <div className="flex h-full gap-2 overflow-x-auto px-2 pb-2">
            {effectiveTargets.map((t) => (
              <ConsolePane
                key={t.vpsId}
                target={t}
                broadcastRef={broadcastRef}
                broadcastInput={broadcastInput}
                onClose={closePane}
                onRegistryChange={() => setActiveCount(consoleSessions.size)}
                visible={open}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
