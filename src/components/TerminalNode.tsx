import { useCallback, useEffect, useRef, useState } from "react";
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
import { useTerminalClipboard } from "../hooks/useTerminalClipboard";
import { shellQuote } from "../lib/terminalClipboard";
import { onOsDropHover, onOsFilesDropped } from "../hooks/useOsFileDrop";
import { onInternalDrop, useDragStore } from "../stores/dragStore";
import { bytesToB64 } from "../lib/tauri";
import { GitBranchBadge, useGitBranch } from "../hooks/useGitBranch";

/** A file that was just put on the server, shown as a dismissible chip. */
interface DroppedChip {
  name: string;
  path: string;
  size: number;
  preview?: string;
  isImage: boolean;
}

function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

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
  const status = (info?.status ?? "connecting") as ConnState;
  const gitInfo = useGitBranch({
    enabled: status === "connected",
    path: info?.cwd,
    vpsId: data.vpsId,
  });
  // Keep session store in sync so other UI (and agent canvas context) can see the branch.
  useEffect(() => {
    setInfo(id, {
      gitBranch: gitInfo?.branch ?? null,
      gitDirty: gitInfo?.dirty ?? false,
    });
  }, [gitInfo, id, setInfo]);
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

    let initialFont = 13;
    try {
      const n = Number(localStorage.getItem("xconsole-term-font"));
      if (n >= 10 && n <= 22) initialFont = n;
    } catch {
      /* ignore */
    }
    const term = new Terminal({
      fontFamily:
        '"Cascadia Code", "JetBrains Mono", "Fira Code", Consolas, monospace',
      fontSize: initialFont,
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
        const msg = String(e);
        // A refused host key is a decision, not a blip. Retrying it five times only
        // repeated the alarm and buried the fingerprints in a wall of identical
        // messages — and the answer is never "wait": either the server's keys really
        // changed and the old pin has to go, or something is impersonating it.
        if (msg.includes("host key mismatch")) {
          term.writeln(`\r\n\x1b[31m${msg.replace(/\n/g, "\r\n")}\x1b[0m`);
          setInfo(id, { status: "error", error: "Host key mismatch — not reconnecting." });
          return;
        }
        // The server may still be booting — keep retrying with backoff.
        term.writeln(`\r\n\x1b[33m… connection failed: ${msg} — retrying\x1b[0m`);
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

  const mismatch = info?.hostKey === "mismatch";
  const canReconnect = status === "disconnected" || status === "error";

  // ----- Sending text to the shell -----
  const sendText = useCallback((text: string) => {
    const sid = sessionIdRef.current;
    if (sid) api.sshWrite(sid, strToB64(text)).catch(() => {});
  }, []);

  /** Type a path at the prompt, quoted and with a trailing space so it reads naturally. */
  const insertPath = useCallback(
    (remotePath: string) => {
      sendText(`${shellQuote(remotePath)} `);
      termRef.current?.focus();
    },
    [sendText],
  );

  // ----- Files arriving from outside -----
  const dropId = `terminal:${id}`;
  const [chips, setChips] = useState<DroppedChip[]>([]);
  const [uploading, setUploading] = useState<string | null>(null);
  const [dropActive, setDropActive] = useState(false);

  const upload = useCallback(
    async (localPaths: string[], inline: { name: string; content_b64: string }[]) => {
      const count = localPaths.length + inline.length;
      setUploading(`Uploading ${count} file${count === 1 ? "" : "s"}…`);
      try {
        // Land next to whatever the shell is looking at, so the path just typed is the
        // one a relative command would find. Falls back to the home directory.
        const dir = useSessionStore.getState().sessions[id]?.cwd || ".";
        const done = await api.terminalUpload(data.vpsId, dir, localPaths, inline);
        setChips((c) => [
          ...done.map((u) => ({
            name: u.name,
            path: u.path,
            size: u.size,
            preview: u.preview_b64 ?? undefined,
            isImage: u.is_image,
          })),
          ...c,
        ].slice(0, 6));
        // One file: type it straight away, which is the whole point of the gesture.
        // Several: leave it to the chips, since typing six paths is rarely what was meant.
        if (done.length === 1) insertPath(done[0].path);
        setUploading(null);
      } catch (e) {
        setUploading(`Upload failed: ${String(e)}`);
        window.setTimeout(() => setUploading(null), 4000);
      }
    },
    [data.vpsId, id, insertPath],
  );

  // Files dragged in from Explorer land as a window event, so each node filters by target.
  useEffect(() => {
    const offDrop = onOsFilesDropped((target, paths) => {
      if (target !== dropId) return;
      void upload(paths, []);
    });
    const offHover = onOsDropHover((t) => setDropActive(t === dropId));
    return () => {
      offDrop();
      offHover();
    };
  }, [dropId, upload]);

  // A remote file dragged from an SFTP panel: nothing to transfer, just type its path.
  const internalOver = useDragStore((s) => (s.over === dropId ? s.drag : null));
  useEffect(() => {
    return onInternalDrop(dropId, (payload) => {
      if (payload.kind !== "remote-file" || !payload.path) return;
      insertPath(payload.path);
    });
  }, [dropId, insertPath]);

  const hint = useTerminalClipboard({
    term: termRef,
    host: containerRef,
    send: sendText,
    onImage: (png) => {
      const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      void upload([], [{ name: `screenshot-${stamp}.png`, content_b64: bytesToB64(png) }]);
    },
  });

  const showDropOverlay = dropActive || !!internalOver;

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
        // Always mounted, not just when selected: needing to click a node before you
        // could resize it was the whole reason edges were "hard to grab". The handles
        // stay invisible until hover — see .xc-resize-* in styles.css, which also gives
        // them a hit area far wider than the 1px line they draw.
        isVisible
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
        <button
          type="button"
          className="truncate font-medium text-gray-200 hover:text-white"
          data-tooltip="Click to copy server name"
          onClick={(e) => {
            e.stopPropagation();
            void navigator.clipboard.writeText(String(data.name ?? ""));
          }}
        >
          {data.name}
        </button>
        <button
          type="button"
          className="truncate text-gray-500 hover:text-gray-300"
          data-tooltip="Click to copy host"
          onClick={(e) => {
            e.stopPropagation();
            void navigator.clipboard.writeText(String(data.host ?? ""));
          }}
        >
          {data.host}
        </button>
        {info?.cwd && (
          <button
            type="button"
            className="max-w-[120px] truncate font-mono text-[10px] text-gray-600 hover:text-gray-300"
            data-tooltip={`${info.cwd} — click to copy`}
            onClick={(e) => {
              e.stopPropagation();
              void navigator.clipboard.writeText(info.cwd!);
            }}
          >
            {info.cwd}
          </button>
        )}
        <GitBranchBadge info={gitInfo} />
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
            className="rounded px-1 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            data-tooltip="Smaller font"
            onClick={(e) => {
              e.stopPropagation();
              const term = termRef.current;
              if (!term) return;
              const next = Math.max(10, (term.options.fontSize as number) - 1);
              term.options.fontSize = next;
              try {
                localStorage.setItem("xconsole-term-font", String(next));
              } catch {
                /* ignore */
              }
              fitRef.current?.fit();
            }}
          >
            A−
          </button>
          <button
            className="rounded px-1 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            data-tooltip="Larger font"
            onClick={(e) => {
              e.stopPropagation();
              const term = termRef.current;
              if (!term) return;
              const next = Math.min(22, (term.options.fontSize as number) + 1);
              term.options.fontSize = next;
              try {
                localStorage.setItem("xconsole-term-font", String(next));
              } catch {
                /* ignore */
              }
              fitRef.current?.fit();
            }}
          >
            A+
          </button>
          <button
            className="rounded px-1.5 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            data-tooltip="Clear scrollback (does not kill the shell)"
            onClick={(e) => {
              e.stopPropagation();
              termRef.current?.clear();
            }}
          >
            ⌫
          </button>
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

      {/* Terminal body: nodrag/nowheel so typing & scrolling don't move the canvas.
          data-drop makes it a target for both Explorer files and internal drags. */}
      <div className="relative min-h-0 flex-1" data-drop={dropId}>
        <div
          ref={containerRef}
          className="xterm-host nodrag nowheel h-full w-full"
          onClick={() => termRef.current?.focus()}
        />

        {showDropOverlay && (
          <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center border-2 border-dashed border-[var(--accent)] bg-[var(--accent)]/10">
            <span className="rounded bg-[var(--surface)] px-2 py-1 text-xs text-gray-100">
              {internalOver
                ? `Insert path — ${internalOver.label}`
                : "Upload here and insert the path"}
            </span>
          </div>
        )}

        {(hint || uploading) && (
          <div className="pointer-events-none absolute bottom-2 left-1/2 z-20 -translate-x-1/2 whitespace-nowrap rounded bg-black/80 px-2 py-1 text-[11px] text-gray-200">
            {uploading ?? hint}
          </div>
        )}
      </div>

      {chips.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 border-t border-[var(--border)] bg-[var(--surface)] px-2 py-1.5">
          {chips.map((c) => (
            <button
              key={c.path}
              className="flex max-w-[190px] items-center gap-1.5 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-1 text-left text-[10px] text-gray-300 hover:border-[var(--accent)]"
              data-tooltip={`${c.path} — click to insert the path again`}
              onClick={(e) => {
                e.stopPropagation();
                insertPath(c.path);
              }}
            >
              {c.preview ? (
                <img
                  src={`data:image/*;base64,${c.preview}`}
                  alt=""
                  className="h-7 w-7 shrink-0 rounded object-cover"
                />
              ) : (
                <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded bg-[var(--surface)] text-gray-500">
                  {c.isImage ? "▣" : "≡"}
                </span>
              )}
              <span className="min-w-0">
                <span className="block truncate text-gray-200">{c.name}</span>
                <span className="block text-gray-500">{humanSize(c.size)}</span>
              </span>
            </button>
          ))}
          <button
            className="ml-auto rounded px-1.5 py-1 text-[10px] text-gray-500 hover:text-gray-300"
            data-tooltip="Clear this list (the files stay on the server)"
            onClick={(e) => {
              e.stopPropagation();
              setChips([]);
            }}
          >
            clear
          </button>
        </div>
      )}

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
