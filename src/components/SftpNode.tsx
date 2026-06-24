import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  Handle,
  NodeResizer,
  Position,
  useReactFlow,
  useStore,
  type NodeProps,
} from "@xyflow/react";
import { api, type SftpEntry } from "../lib/tauri";
import { useCanvasStore, type SftpNode as SftpNodeType } from "../stores/canvasStore";
import { useSessionStore } from "../stores/sessionStore";
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
  const [status, setStatus] = useState<ConnState>("connecting");
  const [error, setError] = useState<string | null>(null);
  const [path, setPath] = useState("/");
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
      setError(String(e));
      setStatus("error");
      return null;
    } finally {
      setLoading(false);
    }
  }, []);

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
    const sid = sessionRef.current;
    if (!sid) return;
    void loadDir(sid, path);
    void fetchTreeDir(sid, path);
    void fetchTreeDir(sid, "/");
  }, [path, loadDir, fetchTreeDir]);

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
          setError(String(e));
          setStatus("error");
        }
      }
    })();

    return () => {
      mounted = false;
      const sid = sessionRef.current;
      if (sid) api.sftpDisconnect(sid).catch(() => {});
      sessionRef.current = null;
    };
  }, [data.vpsId, loadDir, fetchTreeDir]);

  useEffect(() => {
    if (!followTerminal || !linkedTerminalId || !terminalCwd) return;
    const sid = sessionRef.current;
    if (!sid || terminalCwd === lastSyncedCwd.current) return;
    lastSyncedCwd.current = terminalCwd;
    void loadDir(sid, terminalCwd);
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
  }, [followTerminal, linkedTerminalId, terminalCwd, loadDir]);

  const closeNode = () => {
    const sid = sessionRef.current;
    if (sid) api.sftpDisconnect(sid).catch(() => {});
    sessionRef.current = null;
    removeNode(id);
  };

  const openEntry = (entry: SftpEntry) => {
    const sid = sessionRef.current;
    if (!sid || !entry.is_dir) return;
    void loadDir(sid, entry.path);
  };

  const goUp = () => {
    const sid = sessionRef.current;
    if (!sid) return;
    void loadDir(sid, parentPath(path));
  };

  const refresh = () => refreshListing();

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
    const sid = sessionRef.current;
    if (!sid || !pathInput.trim()) return;
    void loadDir(sid, pathInput.trim());
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

  const selectTreeDir = (dir: string) => {
    const sid = sessionRef.current;
    if (!sid) return;
    void loadDir(sid, dir);
  };

  const downloadFile = async (entry: SftpEntry) => {
    const sid = sessionRef.current;
    if (!sid || entry.is_dir) return;
    try {
      const b64 = await api.sftpDownload(sid, entry.path);
      const bin = atob(b64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i += 1) bytes[i] = bin.charCodeAt(i);
      const blob = new Blob([bytes]);
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = entry.name;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleFollow = () => {
    const next = !followTerminal;
    updateNodeData(id, { followTerminal: next });
    if (next && terminalCwd && sessionRef.current) {
      lastSyncedCwd.current = null;
      void loadDir(sessionRef.current, terminalCwd);
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
        isVisible={selected}
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
          <div className="border-b border-red-900/40 bg-red-950/30 px-2 py-1 text-[10px] text-red-300">
            {error}
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
                    {!entry.is_dir && (
                      <button
                        type="button"
                        className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-gray-500 opacity-0 hover:bg-[var(--border)] hover:text-gray-200 group-hover:opacity-100"
                        onClick={() => void downloadFile(entry)}
                      >
                        ↓
                      </button>
                    )}
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
          onDownload={(e) => void downloadFile(e)}
          onProperties={(e) => setPropsEntry(e)}
          onRename={(e) => void handleRename(e)}
          onDelete={(e) => void handleDelete(e)}
          onCopyPath={(p) => void handleCopyPath(p)}
          onNewFolder={() => void handleNewFolder()}
          onNewFile={() => void handleNewFile()}
          onRefresh={refresh}
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
