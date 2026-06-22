import { useEffect, useRef } from "react";
import { Handle, NodeResizer, Position, useReactFlow, type NodeProps } from "@xyflow/react";
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
import { cwdFromCdInput, extractCwdFromOutput } from "../lib/terminalCwd";
import { useXtermScaleFix } from "../hooks/useXtermScaleFix";
import { useCanvasStore, type TermNode } from "../stores/canvasStore";
import { useSessionStore, type ConnState } from "../stores/sessionStore";

const THEME = {
  background: "#0b0f17",
  foreground: "#d6deeb",
  cursor: "#7aa2f7",
  selectionBackground: "#283457",
  black: "#1b1f2a",
  brightBlack: "#444b6a",
  red: "#f7768e",
  green: "#9ece6a",
  yellow: "#e0af68",
  blue: "#7aa2f7",
  magenta: "#bb9af7",
  cyan: "#7dcfff",
  white: "#a9b1d6",
};

const STATUS_COLOR: Record<ConnState, string> = {
  connecting: "#e0af68",
  connected: "#9ece6a",
  reconnecting: "#e0af68",
  disconnected: "#6b7280",
  error: "#f7768e",
};

export function TerminalNode({ id, data, selected }: NodeProps<TermNode>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string | null>(null);

  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const { fitView } = useReactFlow();
  const setInfo = useSessionStore((s) => s.setInfo);
  const removeInfo = useSessionStore((s) => s.remove);
  const info = useSessionStore((s) => s.sessions[id]);

  useXtermScaleFix(containerRef, fitRef);

  // ----- Terminal lifecycle (runs once) -----
  useEffect(() => {
    let mounted = true;
    const unlisteners: UnlistenFn[] = [];

    const term = new Terminal({
      fontFamily:
        '"Cascadia Code", "JetBrains Mono", "Fira Code", Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      scrollback: 5000,
      theme: THEME,
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

    const attach = async (sessionId: string) => {
      sessionIdRef.current = sessionId;
      unlisteners.push(
        await onSessionOutput(sessionId, (bytes) => {
          term.write(bytes);
          const text = new TextDecoder().decode(bytes);
          const cwd = extractCwdFromOutput(text);
          if (cwd) setInfo(id, { cwd });
        }),
      );
      unlisteners.push(
        await onSessionStatus(sessionId, (st) =>
          setInfo(id, { status: statusMap[st.kind] ?? "disconnected" }),
        ),
      );
      const replay = await api.sshReplay(sessionId);
      if (replay) term.write(b64ToBytes(replay));
    };

    (async () => {
      try {
        // Reattach to a still-living background session (e.g. after a workspace
        // switch) so a running process like htop survives.
        const existing = useSessionStore.getState().sessions[id];
        if (existing?.sessionId) {
          const replay = await api.sshReplay(existing.sessionId).catch(() => null);
          if (!mounted) return;
          if (replay !== null) {
            setInfo(id, { status: "connected", sessionId: existing.sessionId });
            await attach(existing.sessionId);
            return;
          }
        }

        setInfo(id, { status: "connecting" });
        const outcome = await api.sshConnect(data.vpsId, cols, rows);
        if (!mounted) {
          await api.sshDisconnect(outcome.session_id);
          return;
        }
        setInfo(id, {
          sessionId: outcome.session_id,
          status: "connected",
          hostKey: outcome.host_key,
        });
        await attach(outcome.session_id);
      } catch (e) {
        if (mounted) setInfo(id, { status: "error", error: String(e) });
        term.writeln(`\r\n\x1b[31mConnection failed: ${String(e)}\x1b[0m`);
      }
    })();

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
      dataSub.dispose();
      ro.disconnect();
      unlisteners.forEach((u) => u());
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Explicit close: tear down the SSH session and remove the node.
  const closeNode = () => {
    const sid = sessionIdRef.current;
    if (sid) api.sshDisconnect(sid).catch(() => {});
    removeInfo(id);
    removeNode(id);
  };

  const status = info?.status ?? "connecting";
  const mismatch = info?.hostKey === "mismatch";

  return (
    <div
      className={`flex h-full w-full flex-col overflow-hidden rounded-lg border bg-[#0b0f17] shadow-lg ${
        selected ? "border-blue-500" : "border-[#1f2737]"
      }`}
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
        className="flex cursor-move items-center gap-2 border-b border-[#1f2737] bg-[#11161f] px-3 py-1.5 text-xs"
        onDoubleClick={() => {
          focus(id);
          fitView({ nodes: [{ id }], duration: 300, padding: 0.1 });
        }}
      >
        <span
          className="inline-block h-2.5 w-2.5 rounded-full"
          style={{ background: STATUS_COLOR[status] }}
          title={status}
        />
        <span className="truncate font-medium text-gray-200">{data.name}</span>
        <span className="truncate text-gray-500">{data.host}</span>
        {info?.cwd && (
          <span
            className="max-w-[120px] truncate font-mono text-[10px] text-gray-600"
            title={info.cwd}
          >
            {info.cwd}
          </span>
        )}
        {info?.hostKey === "pinned_on_first_use" && (
          <span
            className="rounded bg-amber-900/50 px-1 text-[10px] text-amber-300"
            title="Host key pinned on first connection"
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
          <button
            className="rounded px-1.5 py-0.5 text-gray-400 hover:bg-[#1f2737] hover:text-gray-200"
            title="Close terminal"
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
        className="!h-3 !w-3 !border-2 !border-cyan-400 !bg-[#0b0f17]"
        title="Connect from SFTP to sync path"
      />
    </div>
  );
}
