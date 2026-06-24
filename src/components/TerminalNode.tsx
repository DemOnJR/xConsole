import { useEffect, useRef } from "react";
import { Handle, NodeResizer, Position, useReactFlow, useStore, type NodeProps } from "@xyflow/react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  b64ToBytes,
  onCanvasCommand,
  onSessionOutput,
  onSessionStatus,
  strToB64,
} from "../lib/tauri";
import { cwdFromCdInput, extractCwdFromOutput } from "../lib/terminalCwd";
import { useXtermScaleFix } from "../hooks/useXtermScaleFix";
import { useCanvasStore, type TermNode } from "../stores/canvasStore";
import { useSessionStore, type ConnState } from "../stores/sessionStore";
import { useThemeStore } from "../stores/themeStore";

const STATUS_COLOR: Record<ConnState, string> = {
  connecting: "#e0af68",
  connected: "#9ece6a",
  reconnecting: "#e0af68",
  disconnected: "#6b7280",
  error: "#f7768e",
};

export function TerminalNode({ id, data, selected, dragging }: NodeProps<TermNode>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  // Auto-reconnect bookkeeping (e.g. after the server reboots).
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptsRef = useRef(0);
  const closingRef = useRef(false);
  const reconnectFnRef = useRef<() => void>(() => {});

  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const { fitView } = useReactFlow();
  const setInfo = useSessionStore((s) => s.setInfo);
  const removeInfo = useSessionStore((s) => s.remove);
  const info = useSessionStore((s) => s.sessions[id]);
  const themeId = useThemeStore((s) => s.themeId);
  const customVars = useThemeStore((s) => s.customVars);
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  // In freeform mode the terminal scales with the canvas (zoom out → it and its
  // font shrink, like any node). In tile/snap modes we counter the canvas zoom so
  // the terminal keeps a constant on-screen size (and selection stays exact).
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);
  // On-screen scale of this terminal. In freeform it scales with the canvas zoom;
  // in fixed layouts the node counter-scales (below) so it stays 1:1. The scale-fix
  // hook reads this live to keep selection/hit-testing aligned at any zoom.
  const scaleRef = useRef(1);
  scaleRef.current = freeform ? zoom : 1;

  // Recolor the live terminal whenever the active theme changes.
  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = useThemeStore.getState().xterm();
  }, [themeId, customVars]);

  // Keep mouse selection/hit-testing aligned with the glyphs at any canvas zoom.
  // The terminal still scales visually (text shrinks in freeform); we only correct
  // xterm's internal cell math by the live scale. No-op when the terminal is 1:1.
  useXtermScaleFix(termRef, scaleRef);

  // ----- Terminal lifecycle (runs once) -----
  useEffect(() => {
    let mounted = true;
    let disposed = false;
    // Listeners for the CURRENT ssh session (replaced on each reconnect).
    let sessionUnlisteners: UnlistenFn[] = [];

    const MAX_RECONNECT = 15;

    const term = new Terminal({
      fontFamily:
        '"Cascadia Code", "JetBrains Mono", "Fira Code", Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      scrollback: 5000,
      theme: useThemeStore.getState().xterm(),
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    termRef.current = term;
    fitRef.current = fit;

    if (containerRef.current) term.open(containerRef.current);
    safeFit();

    const cols = term.cols || 80;
    const rows = term.rows || 24;

    const statusMap: Record<string, ConnState> = {
      Connecting: "connecting",
      Connected: "connected",
      Reconnecting: "reconnecting",
      Disconnected: "disconnected",
      Error: "error",
    };

    const clearSessionListeners = () => {
      sessionUnlisteners.forEach((u) => u());
      sessionUnlisteners = [];
    };

    // Schedule an automatic reconnect with a small backoff. Triggered when a live
    // session drops (server reboot, network blip) or an attempt fails.
    const scheduleReconnect = () => {
      if (disposed || closingRef.current || reconnectTimerRef.current != null) return;
      if (reconnectAttemptsRef.current >= MAX_RECONNECT) {
        setInfo(id, {
          status: "error",
          error: "Couldn't reconnect automatically — click ↻ to retry.",
        });
        return;
      }
      const attempt = reconnectAttemptsRef.current;
      const delay = Math.min(2000 + attempt * 1500, 8000);
      setInfo(id, { status: "reconnecting" });
      reconnectTimerRef.current = window.setTimeout(() => {
        reconnectTimerRef.current = null;
        reconnectAttemptsRef.current = attempt + 1;
        void connect(true);
      }, delay);
    };

    const attach = async (sessionId: string) => {
      sessionIdRef.current = sessionId;
      sessionUnlisteners.push(
        await onSessionOutput(sessionId, (bytes) => {
          term.write(bytes);
          const text = new TextDecoder().decode(bytes);
          const cwd = extractCwdFromOutput(text);
          if (cwd) setInfo(id, { cwd });
        }),
      );
      sessionUnlisteners.push(
        await onSessionStatus(sessionId, (st) => {
          const s = statusMap[st.kind] ?? "disconnected";
          setInfo(id, { status: s });
          // A live session dropped — reconnect (unless the user is closing it).
          if ((s === "disconnected" || s === "error") && !closingRef.current && !disposed) {
            scheduleReconnect();
          }
        }),
      );
    };

    const connect = async (isReconnect: boolean) => {
      if (disposed || closingRef.current) return;
      clearSessionListeners();
      try {
        if (!isReconnect) {
          // Reattach to a still-living background session (e.g. after a workspace
          // switch) so a running process like htop survives.
          const existing = useSessionStore.getState().sessions[id];
          if (existing?.sessionId) {
            const replay = await api.sshReplay(existing.sessionId).catch(() => null);
            if (!mounted) return;
            if (replay !== null) {
              setInfo(id, { status: "connected", sessionId: existing.sessionId });
              await attach(existing.sessionId);
              if (replay) term.write(b64ToBytes(replay));
              reconnectAttemptsRef.current = 0;
              return;
            }
          }
        }
        setInfo(id, { status: isReconnect ? "reconnecting" : "connecting" });
        const outcome = await api.sshConnect(data.vpsId, cols, rows);
        if (!mounted || disposed) {
          await api.sshDisconnect(outcome.session_id).catch(() => {});
          return;
        }
        setInfo(id, {
          sessionId: outcome.session_id,
          status: "connected",
          hostKey: outcome.host_key,
        });
        await attach(outcome.session_id);
        const replay = await api.sshReplay(outcome.session_id).catch(() => null);
        if (replay) term.write(b64ToBytes(replay));
        if (isReconnect) term.writeln("\r\n\x1b[32m✓ reconnected\x1b[0m");
        reconnectAttemptsRef.current = 0;
      } catch (e) {
        if (!mounted || disposed) return;
        // The server may still be booting — keep retrying with backoff.
        term.writeln(`\r\n\x1b[33m… connection failed: ${String(e)} — retrying\x1b[0m`);
        scheduleReconnect();
      }
    };

    // Manual / agent-driven reconnect: drop the old session and connect fresh.
    reconnectFnRef.current = () => {
      if (disposed) return;
      if (reconnectTimerRef.current != null) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      reconnectAttemptsRef.current = 0;
      clearSessionListeners();
      const old = sessionIdRef.current;
      sessionIdRef.current = null;
      if (old) api.sshDisconnect(old).catch(() => {});
      void connect(true);
    };

    void connect(false);

    let inputBuf = "";
    const dataSub = term.onData((d) => {
      const sid = sessionIdRef.current;
      if (sid) api.sshWrite(sid, strToB64(d)).catch(() => {});

      inputBuf += d;
      if (d.includes("\r") || d.includes("\n")) {
        const line = inputBuf;
        inputBuf = "";
        const cur = useSessionStore.getState().sessions[id]?.cwd;
        const next = cwdFromCdInput(line, cur);
        if (next) setInfo(id, { cwd: next });
      }
    });

    const ro = new ResizeObserver(() => safeFit());
    if (containerRef.current) ro.observe(containerRef.current);

    function safeFit() {
      try {
        fit.fit();
        const sid = sessionIdRef.current;
        if (sid) api.sshResize(sid, term.cols, term.rows).catch(() => {});
      } catch {
        /* container not measurable yet */
      }
    }

    return () => {
      // Detach the UI but KEEP the backend session alive so switching workspaces
      // (which unmounts this node) doesn't kill a running process. The session is
      // only closed via the explicit close button (see `closeNode`).
      mounted = false;
      disposed = true;
      if (reconnectTimerRef.current != null) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      dataSub.dispose();
      ro.disconnect();
      clearSessionListeners();
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Agent-driven reconnect (canvas_refresh): match this node or its server.
  useEffect(() => {
    let un: (() => void) | undefined;
    onCanvasCommand((cmd) => {
      if (
        cmd.action === "reconnect" &&
        (cmd.node_id === id || (!cmd.node_id && cmd.vps_id === data.vpsId))
      ) {
        reconnectFnRef.current();
      }
    }).then((u) => (un = u));
    return () => un?.();
  }, [id, data.vpsId]);

  // Explicit close: tear down the SSH session and remove the node.
  const closeNode = () => {
    closingRef.current = true; // don't auto-reconnect a deliberately closed session
    if (reconnectTimerRef.current != null) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    const sid = sessionIdRef.current;
    if (sid) api.sshDisconnect(sid).catch(() => {});
    removeInfo(id);
    removeNode(id);
  };

  const status = info?.status ?? "connecting";
  const mismatch = info?.hostKey === "mismatch";
  const canReconnect = status === "disconnected" || status === "error";

  return (
    <div
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-blue-500" : "border-[var(--border)]"}`}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
      onMouseDown={() => focus(id)}
    >
      <NodeResizer
        minWidth={280}
        minHeight={180}
        isVisible={selected}
        lineClassName="!border-blue-500"
        handleClassName="!bg-blue-500"
      />

      {/* Header / drag handle. Double-click = focus mode (zoom into this terminal). */}
      <div
        className="flex cursor-move items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-xs"
        onDoubleClick={() => {
          focus(id);
          fitView({ nodes: [{ id }], duration: 300, padding: 0.1 });
        }}
      >
        <span
          className="inline-block h-2.5 w-2.5 rounded-full"
          style={{ background: STATUS_COLOR[status] }}
          data-tooltip={status}
        />
        <span className="truncate font-medium text-gray-200">{data.name}</span>
        <span className="truncate text-gray-500">{data.host}</span>
        {info?.cwd && (
          <span
            className="max-w-[120px] truncate font-mono text-[10px] text-gray-600"
            data-tooltip={info.cwd}
          >
            {info.cwd}
          </span>
        )}
        {info?.hostKey === "pinned_on_first_use" && (
          <span
            className="rounded bg-amber-900/50 px-1 text-[10px] text-amber-300"
            data-tooltip="Host key pinned on first connection"
          >
            pinned
          </span>
        )}
        {mismatch && (
          <span className="rounded bg-red-900/60 px-1 text-[10px] text-red-300">
            key mismatch
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          {canReconnect && (
            <button
              className="rounded px-1.5 py-0.5 text-amber-300 hover:bg-[var(--border)] hover:text-amber-200"
              data-tooltip="Reconnect (the session dropped)"
              onClick={(e) => {
                e.stopPropagation();
                reconnectFnRef.current();
              }}
            >
              ↻
            </button>
          )}
          <button
            className="rounded px-1.5 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            data-tooltip="Close terminal"
            onClick={(e) => {
              e.stopPropagation();
              closeNode();
            }}
          >
            ✕
          </button>
        </div>
      </div>

      {/* Terminal body: nodrag/nowheel so typing & scrolling don't move the canvas */}
      <div
        ref={containerRef}
        className="xterm-host nodrag nowheel min-h-0 flex-1"
        onClick={() => termRef.current?.focus()}
      />

      <Handle
        type="target"
        position={Position.Right}
        id="path-in"
        className={`!h-3 !w-3 !border-2 !border-cyan-400 !bg-[var(--bg)] !opacity-0 transition-opacity ${
          dragging ? "" : "group-hover:!opacity-100"
        }`}
        data-tooltip="Drag an SFTP panel's dot here to make it follow this terminal's folder"
      />
    </div>
  );
}
