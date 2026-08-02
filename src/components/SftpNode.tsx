import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { startInternalDrag } from "../stores/dragStore";

/**
 * SFTP sessions that outlive their component, keyed by canvas node id.
 *
 * A node unmounts for reasons that are not "the user closed it" — the agent panel
 * expanding to full width, a workspace switch, any parent that re-renders it out of the
 * tree. Tearing the connection down there is what made the file browser and its current
 * directory vanish whenever something unrelated happened. Terminals already worked this
 * way; this brings SFTP in line. Only `closeNode` really disconnects.
 */
const keptSftpSessions = new Map<string, { sessionId: string; path: string }>();
import {
  Handle,
  NodeResizer,
  Position,
  useReactFlow,
  useStore,
  type NodeProps,
} from "@xyflow/react";
import {
  api,
  onExternalEdit,
  type ArchiveFormat,
  type SftpEntry,
} from "../lib/tauri";
import { looksLikeDeadSession } from "../lib/sessionHealth";
import { useSettingsStore } from "../stores/settingsStore";
import { useMouseNavButtons, useNavHistory } from "../hooks/useNavHistory";
import { useCanvasStore, type SftpNode as SftpNodeType } from "../stores/canvasStore";
import { useSessionStore } from "../stores/sessionStore";
import { useTransferStore } from "../stores/transferStore";
import { dialog } from "../stores/dialogStore";
import { ChevronUpIcon, FolderIcon } from "./icons";
import { SftpContextMenu, type SftpMenuState } from "./SftpContextMenu";
import { SftpPermissionsDialog } from "./SftpPermissionsDialog";
import { SftpCodeEditor } from "./SftpCodeEditor";

type ConnState = "connecting" | "connected" | "error" | "disconnected";

const DEFAULT_TREE_W = 130;
const MIN_TREE_W = 72;
const MAX_TREE_W = 520;

const STATUS_COLOR: Record<ConnState, string> = {
  connecting: "#e0af68",
  connected: "#9ece6a",
  disconnected: "#6b7280",
  error: "#f7768e",
};

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function parentPath(path: string): string {
  const p = path.replace(/\/+$/, "") || "/";
  if (p === "/") return "/";
  const idx = p.lastIndexOf("/");
  return idx <= 0 ? "/" : p.slice(0, idx);
}

function joinRemotePath(base: string, name: string): string {
  const b = base.replace(/\/+$/, "") || "";
  return b ? `${b}/${name}` : `/${name}`;
}

function parentDirOf(filePath: string): string {
  const idx = filePath.lastIndexOf("/");
  return idx <= 0 ? "/" : filePath.slice(0, idx);
}

function pathSegments(path: string): string[] {
  if (path === "/") return [];
  return path.replace(/\/+$/, "").split("/").filter(Boolean);
}

interface TreeNodeProps {
  name: string;
  path: string;
  depth: number;
  currentPath: string;
  expanded: Set<string>;
  loadingPaths: Set<string>;
  onToggle: (path: string) => void;
  onSelect: (path: string) => void;
  renderChildren: (path: string, depth: number) => ReactNode;
}

function TreeNode({
  name,
  path,
  depth,
  currentPath,
  expanded,
  loadingPaths,
  onToggle,
  onSelect,
  renderChildren,
}: TreeNodeProps) {
  const isOpen = expanded.has(path);
  const isActive = currentPath === path || currentPath.startsWith(`${path}/`);

  return (
    <div>
      <div
        className={`flex items-center gap-0.5 rounded px-1 py-0.5 hover:bg-[var(--surface)] ${
          isActive ? "bg-cyan-950/40 text-cyan-300" : "text-gray-400"
        }`}
        style={{ paddingLeft: `${depth * 10 + 4}px` }}
      >
        <button
          type="button"
          className="w-3 shrink-0 text-[10px] text-gray-600 hover:text-gray-300"
          onClick={() => onToggle(path)}
        >
          {loadingPaths.has(path) ? "…" : isOpen ? "▾" : "▸"}
        </button>
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-1 truncate text-left text-[10px]"
          onClick={() => onSelect(path)}
          onDoubleClick={() => onToggle(path)}
        >
          <span className="text-cyan-500/80">📁</span>
          <span className="truncate">{name}</span>
        </button>
      </div>
      {isOpen && renderChildren(path, depth + 1)}
    </div>
  );
}

export function SftpNode({ id, data, selected, dragging }: NodeProps<SftpNodeType>) {
  const focus = useCanvasStore((s) => s.focus);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const updateNodeData = useCanvasStore((s) => s.updateNodeData);
  const { fitView } = useReactFlow();

  const linkedTerminalId = data.linkedTerminalId;
  const followTerminal = data.followTerminal ?? !!linkedTerminalId;
  const terminalCwd = useSessionStore((s) =>
    linkedTerminalId ? s.sessions[linkedTerminalId]?.cwd : undefined,
  );
  const setSessionInfo = useSessionStore((s) => s.setInfo);
  const removeSessionInfo = useSessionStore((s) => s.remove);

  const sessionRef = useRef<string | null>(null);
  const lastSyncedCwd = useRef<string | null>(null);
  /** Scopes the mouse back/forward buttons to this panel. */
  const panelRef = useRef<HTMLDivElement | null>(null);
  const [status, setStatus] = useState<ConnState>("connecting");
  const [error, setError] = useState<string | null>(null);
  const [path, setPath] = useState("/");
  // Mirror of `path` readable from the unmount cleanup, which closes over stale state.
  /// The last failure text, so a caller can tell a dead link from a refusal.
  const lastErrorRef = useRef<string | null>(null);
  const pathRef = useRef(path);
  pathRef.current = path;
  const [pathInput, setPathInput] = useState("/");
  const [entries, setEntries] = useState<SftpEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [showTree, setShowTree] = useState(true);
  const [treeWidth, setTreeWidth] = useState(DEFAULT_TREE_W);
  const [treeResizing, setTreeResizing] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(["/"]));
  const [treeCache, setTreeCache] = useState<Record<string, SftpEntry[]>>({});
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(() => new Set());
  const [menu, setMenu] = useState<SftpMenuState | null>(null);
  const [propsEntry, setPropsEntry] = useState<SftpEntry | null>(null);
  const [editEntry, setEditEntry] = useState<SftpEntry | null>(null);

  const loadDir = useCallback(async (sessionId: string, dir: string) => {
    setLoading(true);
    setError(null);
    try {
      const out = await api.sftpList(sessionId, dir);
      setPath(out.path);
      setPathInput(out.path);
      setEntries(out.entries);
      setStatus("connected");
      return out;
    } catch (e) {
      // Kept for the caller: whether this was a dead link or a refusal decides between
      // reconnecting and showing the message, and the distinction is in the text.
      lastErrorRef.current = String(e);
      setError(String(e));
      setStatus("error");
      return null;
    } finally {
      setLoading(false);
    }
  }, []);

  /// Throw away the current session and open a new one, landing back where the user was.
  ///
  /// The panel used to hold one session id for its whole life. When the link dropped, that
  /// id stayed in place and every call after it failed the same way — the only way out was
  /// to close the panel and open another, because closing is the one path that clears the
  /// remembered session.
  const reconnect = useCallback(async (): Promise<string | null> => {
    const dead = sessionRef.current;
    sessionRef.current = null;
    keptSftpSessions.delete(id);
    // Best effort: if the link is already gone this is a no-op, but if only the SFTP
    // channel died the backend still holds an SSH session worth releasing.
    if (dead) api.sftpDisconnect(dead).catch(() => {});
    setStatus("connecting");
    setError(null);
    try {
      const out = await api.sftpConnect(data.vpsId);
      sessionRef.current = out.session_id;
      keptSftpSessions.set(id, { sessionId: out.session_id, path: pathRef.current });
      setStatus("connected");
      return out.session_id;
    } catch (e) {
      lastErrorRef.current = String(e);
      setError(String(e));
      setStatus("error");
      return null;
    }
  }, [id, data.vpsId]);

  /// List a directory, reconnecting once if the session turns out to be dead.
  ///
  /// This is what every navigation goes through, so a drop is recovered from wherever the
  /// user happens to be rather than only on a manual refresh.
  const openDir = useCallback(
    async (dir: string) => {
      const sid = sessionRef.current;
      if (sid) {
        const out = await loadDir(sid, dir);
        if (out) return out;
        if (!looksLikeDeadSession(lastErrorRef.current)) return null;
      }
      const fresh = await reconnect();
      if (!fresh) return null;
      return loadDir(fresh, dir);
    },
    [loadDir, reconnect],
  );

  const fetchTreeDir = useCallback(async (sessionId: string, dir: string) => {
    setLoadingPaths((s) => new Set(s).add(dir));
    try {
      const out = await api.sftpList(sessionId, dir);
      setTreeCache((c) => ({ ...c, [dir]: out.entries }));
      return out.entries;
    } catch {
      return [];
    } finally {
      setLoadingPaths((s) => {
        const next = new Set(s);
        next.delete(dir);
        return next;
      });
    }
  }, []);

  const refreshListing = useCallback(() => {
    // Through openDir, so Refresh doubles as the manual recovery: pressing it on a panel
    // whose link has dropped reconnects instead of failing the same way again.
    void openDir(path).then((out) => {
      const sid = sessionRef.current;
      if (!out || !sid) return;
      void fetchTreeDir(sid, path);
      void fetchTreeDir(sid, "/");
    });
  }, [path, openDir, fetchTreeDir]);

  // Back/forward through visited directories, driven by the mouse's side buttons as well
  // as the toolbar arrows. `go` deliberately calls loadDir directly rather than
  // navigateTo, so replaying history doesn't push new entries onto it.
  const history = useNavHistory<string>({
    current: path,
    go: useCallback(
      (dir: string) => {
        void openDir(dir);
      },
      [openDir],
    ),
  });
  useMouseNavButtons(panelRef, history);

  // The configured external editor, shown by name in the context menu. Derived from
  // the command so "code --new-window" still reads as "VS Code".
  const editorSetting = useSettingsStore((s) => s.settings["sftp.external_editor"]);
  const externalEditorName = editorSetting?.trim()
    ? /(^|[\\/])code(\.exe|\.cmd)?($|\s)/i.test(editorSetting)
      ? "VS Code"
      : "external editor"
    : null;

  // Report saves pushed back from the external editor — especially refusals, which
  // are the whole point of the guard and must not be silent.
  useEffect(() => {
    let un: (() => void) | undefined;
    void onExternalEdit((e) => {
      if (e.kind === "skipped") setError(e.reason);
      else if (e.kind === "failed") setError(`Save failed: ${e.error}`);
      else if (e.kind === "saved") {
        setError(null);
        refreshListing();
      }
    }).then((u) => (un = u));
    return () => un?.();
  }, [refreshListing]);

  // Publish this panel's live path + status to the session store (keyed by node id)
  // so the agent's per-turn canvas snapshot knows what the user is browsing.
  useEffect(() => {
    setSessionInfo(id, { status, sftpPath: path });
  }, [id, status, path, setSessionInfo]);
  useEffect(() => () => removeSessionInfo(id), [id, removeSessionInfo]);

  useEffect(() => {
    let mounted = true;

    (async () => {
      try {
        // Reattach to the session this node had before it was unmounted. Nodes unmount
        // for reasons that have nothing to do with the user closing the panel — the
        // agent panel expanding, a workspace switch — and reconnecting there dropped
        // the browser back to the home directory every time.
        const previous = keptSftpSessions.get(id);
        if (previous) {
          sessionRef.current = previous.sessionId;
          setStatus("connected");
          // Through openDir: if that remembered session died while the node was
          // unmounted, this reconnects and lands on the same directory instead of
          // presenting a panel that is dead on arrival.
          const out = await openDir(previous.path);
          const sid = sessionRef.current;
          if (out && sid) void fetchTreeDir(sid, "/");
          return;
        }
        setStatus("connecting");
        const out = await api.sftpConnect(data.vpsId);
        if (!mounted) {
          await api.sftpDisconnect(out.session_id);
          return;
        }
        sessionRef.current = out.session_id;
        await loadDir(out.session_id, out.path);
        void fetchTreeDir(out.session_id, "/");
      } catch (e) {
        if (mounted) {
          lastErrorRef.current = String(e);
          setError(String(e));
          setStatus("error");
        }
      }
    })();

    return () => {
      mounted = false;
      // Deliberately NOT disconnecting: the session outlives the component, exactly as
      // a terminal's does. `closePanel` is what actually ends it.
      const sid = sessionRef.current;
      if (sid) keptSftpSessions.set(id, { sessionId: sid, path: pathRef.current });
    };
  }, [id, data.vpsId, loadDir, fetchTreeDir]);

  useEffect(() => {
    if (!followTerminal || !linkedTerminalId || !terminalCwd) return;
    if (terminalCwd === lastSyncedCwd.current) return;
    lastSyncedCwd.current = terminalCwd;
    void openDir(terminalCwd);
    setExpanded((prev) => {
      const next = new Set(prev);
      next.add("/");
      let acc = "";
      for (const seg of pathSegments(terminalCwd)) {
        acc += `/${seg}`;
        next.add(acc);
      }
      return next;
    });
  }, [followTerminal, linkedTerminalId, terminalCwd, openDir]);

  const closeNode = () => {
    const sid = sessionRef.current;
    if (sid) api.sftpDisconnect(sid).catch(() => {});
    sessionRef.current = null;
    keptSftpSessions.delete(id); // an explicit close is the one case that really ends it
    removeNode(id);
  };

  /** Navigate somewhere new, recording it so the mouse's back button can undo it. */
  const navigateTo = useCallback(
    (next: string) => {
      history.visit(next);
      void openDir(next);
    },
    // `history` is stable enough (its callbacks are memoised) that including it here
    // doesn't churn; openDir changes only with the session.
    [history, openDir],
  );

  const openEntry = (entry: SftpEntry) => {
    if (!entry.is_dir) return;
    navigateTo(entry.path);
  };

  const goUp = () => navigateTo(parentPath(path));

  const refresh = () => refreshListing();

  /**
   * Hand the file to the configured external editor. Saves flow back automatically
   * (see the Rust side); this only has to start it and surface the outcome.
   */
  const openExternally = async (entry: SftpEntry) => {
    const sid = sessionRef.current;
    if (!sid || entry.is_dir) return;
    try {
      await api.sftpEditExternal(sid, entry.path);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const showContextMenu = (e: React.MouseEvent, entry: SftpEntry | null) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const handleDelete = async (entry: SftpEntry) => {
    const label = entry.is_dir ? "directory and all contents" : "file";
    if (
      !(await dialog.confirm({
        title: "Delete",
        message: `Delete ${label}?\n\n${entry.path}`,
        danger: true,
        confirmText: "Delete",
      }))
    )
      return;
    try {
      await api.vpsFileDelete(data.vpsId, entry.path, entry.is_dir);
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleRename = async (entry: SftpEntry) => {
    const newName = await dialog.prompt({
      title: "Rename",
      label: "New name",
      defaultValue: entry.name,
      confirmText: "Rename",
    });
    if (!newName?.trim() || newName.trim() === entry.name) return;
    const to = joinRemotePath(parentDirOf(entry.path), newName.trim());
    try {
      await api.vpsFileRename(data.vpsId, entry.path, to);
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleNewFolder = async () => {
    const name = await dialog.prompt({
      title: "New folder",
      label: "Directory name",
      confirmText: "Create",
    });
    if (!name?.trim()) return;
    try {
      await api.vpsFileMkdir(data.vpsId, joinRemotePath(path, name.trim()));
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleNewFile = async () => {
    const name = await dialog.prompt({
      title: "New file",
      label: "File name",
      confirmText: "Create",
    });
    if (!name?.trim()) return;
    try {
      await api.vpsFileTouch(data.vpsId, joinRemotePath(path, name.trim()));
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleCopyPath = async (p: string) => {
    try {
      await navigator.clipboard.writeText(p);
    } catch {
      setError("Could not copy path");
    }
  };

  const navigateToPath = () => {
    if (!pathInput.trim()) return;
    navigateTo(pathInput.trim());
  };

  const toggleTreeDir = async (dir: string) => {
    const sid = sessionRef.current;
    if (!sid) return;

    const willOpen = !expanded.has(dir);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(dir)) next.delete(dir);
      else next.add(dir);
      return next;
    });

    if (willOpen && !treeCache[dir]) {
      await fetchTreeDir(sid, dir);
    }
  };

  const selectTreeDir = (dir: string) => navigateTo(dir);

  /**
   * Download a file or a whole directory.
   *
   * This used to pull the bytes through IPC as base64 and hand them to a browser blob
   * download: capped at 10 MB, no progress, no cancel, and it landed wherever the
   * webview's download directory happened to be. It now queues a real streaming
   * transfer to a folder the user picked, with progress in the transfers panel.
   */
  const downloadEntry = async (entry: SftpEntry) => {
    const sid = sessionRef.current;
    if (!sid) return;
    try {
      await useTransferStore.getState().download(sid, [entry.path]);
    } catch (e) {
      setError(String(e));
    }
  };

  const downloadArchive = async (entry: SftpEntry, format: ArchiveFormat) => {
    const sid = sessionRef.current;
    if (!sid || !entry.is_dir) return;
    try {
      await useTransferStore.getState().downloadArchive(sid, entry.path, format);
    } catch (e) {
      setError(String(e));
    }
  };

  /** Upload into the directory currently shown. */
  const uploadHere = async (localPaths?: string[]) => {
    const sid = sessionRef.current;
    if (!sid) return;
    try {
      await useTransferStore.getState().upload(sid, path, localPaths);
      // The new files won't appear until the listing is re-read.
      window.setTimeout(() => void refreshListing(), 600);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleFollow = () => {
    const next = !followTerminal;
    updateNodeData(id, { followTerminal: next });
    if (next && terminalCwd) {
      lastSyncedCwd.current = null;
      void openDir(terminalCwd);
    }
  };

  const startTreeResize = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = treeWidth;
    setTreeResizing(true);

    const onMove = (ev: MouseEvent) => {
      const next = Math.min(MAX_TREE_W, Math.max(MIN_TREE_W, startW + ev.clientX - startX));
      setTreeWidth(next);
    };
    const onUp = () => {
      setTreeResizing(false);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }, [treeWidth]);

  const renderTreeChildren = (dir: string, depth: number): ReactNode => {
    const entriesForDir = treeCache[dir];
    if (!entriesForDir) return null;
    return entriesForDir
      .filter((e) => e.is_dir)
      .map((entry) => (
        <TreeNode
          key={entry.path}
          name={entry.name}
          path={entry.path}
          depth={depth}
          currentPath={path}
          expanded={expanded}
          loadingPaths={loadingPaths}
          onToggle={toggleTreeDir}
          onSelect={selectTreeDir}
          renderChildren={renderTreeChildren}
        />
      ));
  };

  // Freeform: scale with the canvas (shrink on zoom out). Tile/snap: keep a
  // constant on-screen size by countering the zoom.
  const layoutMode = useCanvasStore((s) => s.layoutMode);
  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);

  return (
    <div
      ref={panelRef}
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-cyan-500" : "border-[var(--border)]"}`}
      style={freeform ? undefined : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }}
      onMouseDown={() => focus(id)}
    >
      <Handle
        type="source"
        position={Position.Left}
        id="path-out"
        className={`!h-3 !w-3 !border-2 !border-cyan-400 !bg-[var(--bg)] !opacity-0 transition-opacity ${
          dragging ? "" : "group-hover:!opacity-100"
        }`}
        data-tooltip="Drag this onto an SSH terminal so this panel follows its folder"
      />

      <NodeResizer
        minWidth={320}
        minHeight={220}
        // Always mounted, not just when selected: needing to click a node before you
        // could resize it was the whole reason edges were "hard to grab". The handles
        // stay invisible until hover — see .xc-resize-* in styles.css, which also gives
        // them a hit area far wider than the 1px line they draw.
        isVisible
        lineClassName="!border-cyan-500"
        handleClassName="!bg-cyan-500"
      />

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
        <FolderIcon size={14} className="shrink-0 text-cyan-400" />
        <span className="truncate font-medium text-gray-200">{data.name}</span>
        <span className="truncate text-gray-500">SFTP · {data.host}</span>
        {linkedTerminalId && (
          <button
            type="button"
            className={`rounded px-1.5 py-0.5 text-[10px] ${
              followTerminal
                ? "bg-cyan-900/50 text-cyan-300"
                : "text-gray-500 hover:bg-[var(--border)]"
            }`}
            data-tooltip={
              followTerminal
                ? "Following SSH path — click to pause"
                : "Paused — click to follow SSH path"
            }
            onClick={(e) => {
              e.stopPropagation();
              toggleFollow();
            }}
          >
            {followTerminal ? "⟳ sync" : "⏸ sync"}
          </button>
        )}
        <div className="ml-auto flex items-center gap-1">
          <button
            className="rounded px-1.5 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            data-tooltip="Close SFTP"
            onClick={(e) => {
              e.stopPropagation();
              closeNode();
            }}
          >
            ✕
          </button>
        </div>
      </div>

      <div className="nodrag nowheel flex min-h-0 flex-1 flex-col">
        <div className="flex items-center gap-1 border-b border-[var(--border)]/80 px-2 py-1">
          {/* Also bound to the mouse's side buttons while the pointer is over this panel. */}
          <button
            type="button"
            className="rounded px-1 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 disabled:opacity-30"
            data-tooltip="Back (mouse button 4)"
            disabled={!history.canBack || loading}
            onClick={history.back}
          >
            ‹
          </button>
          <button
            type="button"
            className="rounded px-1 py-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 disabled:opacity-30"
            data-tooltip="Forward (mouse button 5)"
            disabled={!history.canForward || loading}
            onClick={history.forward}
          >
            ›
          </button>
          <button
            type="button"
            className="rounded p-0.5 text-gray-400 hover:bg-[var(--border)] hover:text-gray-200 disabled:opacity-40"
            data-tooltip="Up"
            disabled={path === "/" || loading}
            onClick={goUp}
          >
            <ChevronUpIcon size={14} />
          </button>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            onClick={refresh}
            disabled={loading}
          >
            Refresh
          </button>
          <button
            type="button"
            className={`rounded px-1.5 py-0.5 text-[10px] ${
              showTree ? "bg-[var(--border)] text-gray-200" : "text-gray-400 hover:bg-[var(--border)]"
            }`}
            data-tooltip="Toggle directory tree"
            onClick={() => setShowTree((v) => !v)}
          >
            Tree
          </button>
          <input
            type="text"
            className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 font-mono text-[10px] text-gray-300 outline-none focus:border-cyan-600"
            value={pathInput}
            spellCheck={false}
            onChange={(e) => setPathInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") navigateToPath();
            }}
            onBlur={() => setPathInput(path)}
            data-tooltip="Remote path — press Enter to go"
          />
        </div>

        {linkedTerminalId && followTerminal && !terminalCwd && (
          <div className="border-b border-amber-900/30 bg-amber-950/20 px-2 py-0.5 text-[10px] text-amber-300/90">
            Linked to terminal — cd in SSH or type a path above
          </div>
        )}

        {error && (
          <div className="flex items-center gap-2 border-b border-red-900/40 bg-red-950/30 px-2 py-1 text-[10px] text-red-300">
            <span className="min-w-0 flex-1 break-words">{error}</span>
            {/* Always offered, even for errors we do not reconnect on automatically:
                whatever the panel got stuck on, the user needs one visible way out that
                is not "close this window and open another". */}
            <button
              type="button"
              className="shrink-0 rounded border border-red-800/60 px-1.5 py-0.5 text-red-200 hover:bg-red-900/40"
              onClick={() =>
                void reconnect().then((sid) => {
                  if (sid) void openDir(pathRef.current || "/");
                })
              }
            >
              Reconnect
            </button>
          </div>
        )}

        {status === "connecting" && (
          <div className="flex flex-1 items-center justify-center text-xs text-gray-500">
            Connecting SFTP…
          </div>
        )}

        {status !== "connecting" && (
          <div className="flex min-h-0 flex-1">
            {showTree && (
              <>
                <div
                  className="shrink-0 overflow-y-auto py-1"
                  style={{ width: treeWidth }}
                >
                  <TreeNode
                    name="/"
                    path="/"
                    depth={0}
                    currentPath={path}
                    expanded={expanded}
                    loadingPaths={loadingPaths}
                    onToggle={toggleTreeDir}
                    onSelect={selectTreeDir}
                    renderChildren={renderTreeChildren}
                  />
                </div>
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-valuenow={treeWidth}
                  data-tooltip="Drag to resize tree"
                  className={`nodrag nowheel shrink-0 cursor-col-resize touch-none select-none ${
                    treeResizing ? "bg-cyan-500/50" : "bg-[var(--border)]/80 hover:bg-cyan-500/40"
                  }`}
                  style={{ width: treeResizing ? 3 : 2 }}
                  onMouseDown={startTreeResize}
                />
              </>
            )}

            <div
              className="min-h-0 flex-1 overflow-y-auto px-1 py-1"
              onContextMenu={(e) => showContextMenu(e, null)}
            >
              {loading && entries.length === 0 ? (
                <div className="px-2 py-4 text-center text-xs text-gray-500">Loading…</div>
              ) : entries.length === 0 ? (
                <div className="px-2 py-4 text-center text-xs text-gray-600">Empty directory</div>
              ) : (
                entries.map((entry) => (
                  <div
                    key={entry.path}
                    className="group flex items-center gap-2 rounded px-2 py-1 hover:bg-[var(--surface)]"
                    onContextMenu={(e) => showContextMenu(e, entry)}
                  >
                    <button
                      type="button"
                      className="flex min-w-0 flex-1 items-center gap-2 text-left"
                      onClick={() => openEntry(entry)}
                      onDoubleClick={() => openEntry(entry)}
                      // Drag onto a terminal to type this path there. Pointer-based,
                      // and only arms after a few pixels of movement, so the click
                      // above still opens the entry.
                      onPointerDown={(e) => {
                        if (e.button !== 0) return;
                        startInternalDrag(e, {
                          kind: "remote-file",
                          vpsId: data.vpsId,
                          path: entry.path,
                          label: entry.name,
                          isDir: entry.is_dir,
                        });
                      }}
                    >
                      <span className={entry.is_dir ? "text-cyan-400" : "text-gray-500"}>
                        {entry.is_dir ? "📁" : "📄"}
                      </span>
                      <span className="truncate text-xs text-gray-200">{entry.name}</span>
                      {!entry.is_dir && (
                        <span className="ml-auto shrink-0 font-mono text-[10px] text-gray-600">
                          {formatSize(entry.size)}
                        </span>
                      )}
                    </button>
                    {/* Folders download too now — the engine walks them. */}
                    <button
                      type="button"
                      className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-gray-500 opacity-0 hover:bg-[var(--border)] hover:text-gray-200 group-hover:opacity-100"
                      data-tooltip={entry.is_dir ? "Download this folder" : "Download"}
                      onClick={() => void downloadEntry(entry)}
                    >
                      ↓
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>
        )}
      </div>

      {menu && (
        <SftpContextMenu
          menu={menu}
          onClose={() => setMenu(null)}
          onOpen={openEntry}
          onEdit={(e) => setEditEntry(e)}
          onEditExternal={(e) => void openExternally(e)}
          onDownload={(e) => void downloadEntry(e)}
          onDownloadArchive={(e, f) => void downloadArchive(e, f)}
          onUpload={() => void uploadHere()}
          onProperties={(e) => setPropsEntry(e)}
          onRename={(e) => void handleRename(e)}
          onDelete={(e) => void handleDelete(e)}
          onCopyPath={(p) => void handleCopyPath(p)}
          onNewFolder={() => void handleNewFolder()}
          onNewFile={() => void handleNewFile()}
          onRefresh={refresh}
          externalEditorName={externalEditorName}
        />
      )}

      {propsEntry && (
        <SftpPermissionsDialog
          entry={propsEntry}
          vpsId={data.vpsId}
          onClose={() => setPropsEntry(null)}
          onApplied={refreshListing}
        />
      )}

      {editEntry && sessionRef.current && (
        <SftpCodeEditor
          sessionId={sessionRef.current}
          entry={editEntry}
          onClose={() => setEditEntry(null)}
          onSaved={refreshListing}
        />
      )}
    </div>
  );
}
