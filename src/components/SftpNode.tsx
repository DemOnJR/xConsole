import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  onInternalDrop,
  startInternalDrag,
  useDragStore,
} from "../stores/dragStore";

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

/**
 * The cut/copy clipboard, deliberately module-level.
 *
 * Copying in one panel and pasting in another is the whole point of having two panels
 * open, and it is what every file manager does. Per-component state would make the
 * clipboard die with the panel it was filled from.
 */
let fileClipboard: { mode: "copy" | "cut"; paths: string[] } | null = null;

/** How long a typeahead buffer survives without another keystroke. */
const TYPEAHEAD_RESET_MS = 1000;
/** Characters typed before the search box opens, so a stray keypress does not open it. */
const TYPEAHEAD_OPEN_AT = 2;
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
  strToB64,
  type ArchiveFormat,
  type LocalFsEntry,
  type SftpEntry,
} from "../lib/tauri";
import { looksLikeDeadSession } from "../lib/sessionHealth";
import { actionTargets, parseExtensions, rangeBetween } from "../lib/selection";
import { onOsDropHover, onOsFilesDropped } from "../hooks/useOsFileDrop";
import { useSettingsStore } from "../stores/settingsStore";
import { useMouseNavButtons, useNavHistory } from "../hooks/useNavHistory";
import { useCanvasStore, type SftpNode as SftpNodeType } from "../stores/canvasStore";
import { useSessionStore } from "../stores/sessionStore";
import { useTransferStore } from "../stores/transferStore";
import { useVpsStore } from "../stores/vpsStore";
import { dialog } from "../stores/dialogStore";
import { ChevronUpIcon, FolderIcon } from "./icons";
import { fileKindFor } from "./fileIcons";
import { SftpContextMenu, type SftpMenuState } from "./SftpContextMenu";
import { SftpPermissionsDialog } from "./SftpPermissionsDialog";
import { SftpCodeEditor } from "./SftpCodeEditor";
import { GitBranchBadge, useGitBranch } from "../hooks/useGitBranch";
import { shellQuote } from "../lib/terminalClipboard";

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
          <span className="shrink-0 text-cyan-500/80">
            <FolderIcon size={12} />
          </span>
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

  /** Paths currently selected, for the bulk actions. */
  const [selection, setSelection] = useState<Set<string>>(() => new Set());
  /** Anchor for shift-click ranges — the last row clicked without shift. */
  const anchorRef = useRef<string | null>(null);
  const [clipboardTick, setClipboardTick] = useState(0);

  const [query, setQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [extInput, setExtInput] = useState("");
  const [recursive, setRecursive] = useState(false);
  /** Recursive-search hits. `null` means "showing the directory", not "no results". */
  const [results, setResults] = useState<string[] | null>(null);
  const [searching, setSearching] = useState(false);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const typeaheadTimer = useRef<number | null>(null);

  /** Files being dragged in from Explorer are over this panel. */
  const [dropActive, setDropActive] = useState(false);
  const dropId = `sftp-${id}`;
  const localDropId = `${dropId}:local`;

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

  // Favorite paths for this host (WinSCP-style bookmarks).
  const bookmarkKey = `xconsole-sftp-bookmarks:${data.vpsId}`;
  const [bookmarks, setBookmarks] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(bookmarkKey) || "[]") as string[];
    } catch {
      return [];
    }
  });
  const toggleBookmark = () => {
    const next = bookmarks.includes(path)
      ? bookmarks.filter((b) => b !== path)
      : [path, ...bookmarks].slice(0, 30);
    setBookmarks(next);
    try {
      localStorage.setItem(bookmarkKey, JSON.stringify(next));
    } catch {
      /* ignore */
    }
  };

  // Dual-pane: local filesystem (left) | remote (right) — WinSCP-style.
  const [dualPane, setDualPane] = useState(false);
  const [localPath, setLocalPath] = useState("");
  const [localEntries, setLocalEntries] = useState<LocalFsEntry[]>([]);
  const [localLoading, setLocalLoading] = useState(false);
  const [localSelection, setLocalSelection] = useState<Set<string>>(() => new Set());
  /** Dual-pane compare: highlight files only-local / only-remote / size-diff. */
  const [compareOn, setCompareOn] = useState(false);
  /** Which dual-pane side receives keyboard selection (Ctrl+A, F5/F6 context). */
  const [activeSide, setActiveSide] = useState<"local" | "remote">("remote");
  type CompareMark = "only-local" | "only-remote" | "diff" | "same";
  const compareMap = useMemo(() => {
    if (!dualPane || !compareOn) return null as Map<string, CompareMark> | null;
    const map = new Map<string, CompareMark>();
    const localByName = new Map(
      localEntries.filter((e) => !e.is_dir).map((e) => [e.name, e]),
    );
    const remoteByName = new Map(
      entries.filter((e) => !e.is_dir).map((e) => [e.name, e]),
    );
    const names = new Set([...localByName.keys(), ...remoteByName.keys()]);
    for (const name of names) {
      const L = localByName.get(name);
      const R = remoteByName.get(name);
      if (L && !R) map.set(name, "only-local");
      else if (!L && R) map.set(name, "only-remote");
      else if (L && R) map.set(name, L.size !== R.size ? "diff" : "same");
    }
    return map;
  }, [dualPane, compareOn, localEntries, entries]);

  const compareStats = useMemo(() => {
    if (!compareMap) return null;
    let onlyLocal = 0;
    let onlyRemote = 0;
    let diff = 0;
    for (const m of compareMap.values()) {
      if (m === "only-local") onlyLocal += 1;
      else if (m === "only-remote") onlyRemote += 1;
      else if (m === "diff") diff += 1;
    }
    return { onlyLocal, onlyRemote, diff };
  }, [compareMap]);

  /** Internal drag hover targets (dual-pane upload/download). */
  const dragOver = useDragStore((s) => s.over);
  const activeDrag = useDragStore((s) => s.drag);
  const remoteDropHighlight =
    dropActive ||
    (dragOver === dropId && activeDrag?.kind === "local-file");
  const localDropHighlight =
    dualPane &&
    dragOver === localDropId &&
    activeDrag?.kind === "remote-file" &&
    activeDrag.vpsId === data.vpsId;

  const remoteGit = useGitBranch({
    enabled: status === "connected",
    path,
    vpsId: data.vpsId,
  });
  const localGit = useGitBranch({
    enabled: dualPane && Boolean(localPath),
    path: localPath || null,
    vpsId: null,
  });
  // Surface remote branch on the session so other UI / agent can read it.
  useEffect(() => {
    setSessionInfo(id, {
      gitBranch: remoteGit?.branch ?? null,
      gitDirty: remoteGit?.dirty ?? false,
      sftpPath: path,
    });
  }, [remoteGit, path, id, setSessionInfo]);

  const loadLocalDir = useCallback(async (dir?: string) => {
    setLocalLoading(true);
    try {
      const out = await api.localFsList(dir || undefined);
      setLocalPath(out.path);
      setLocalEntries(out.entries);
      setLocalSelection(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setLocalLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!dualPane) return;
    if (!localPath) {
      void api
        .localFsHome()
        .then((h) => loadLocalDir(h))
        .catch(() => void loadLocalDir());
    }
  }, [dualPane, localPath, loadLocalDir]);

  const uploadLocalSelection = async () => {
    const sid = sessionRef.current;
    if (!sid || localSelection.size === 0) return;
    // Transfer engine already walks local directories recursively.
    const paths = [...localSelection];
    if (paths.length === 0) return;
    try {
      setError(null);
      await useTransferStore.getState().upload(sid, path, paths);
    } catch (e) {
      setError(String(e));
    }
  };

  /** Open or focus a canvas terminal for this host and `cd` into the chosen directory. */
  const openTerminalHere = (entry: SftpEntry | null) => {
    const dir =
      entry == null
        ? path
        : entry.is_dir
          ? entry.path
          : parentDirOf(entry.path);
    const vps = useVpsStore.getState().vpsList.find((v) => v.id === data.vpsId);
    if (!vps) {
      setError("Server not found in the list.");
      return;
    }
    const nodes = useCanvasStore.getState().nodes;
    const existing = nodes.find(
      (n) => n.type === "terminal" && String(n.data.vpsId) === data.vpsId,
    );
    const termId = existing
      ? existing.id
      : useCanvasStore.getState().addVps(vps);
    useCanvasStore.getState().focus(termId);

    const sendCd = (attempt: number) => {
      const sid = useSessionStore.getState().sessions[termId]?.sessionId;
      const st = useSessionStore.getState().sessions[termId]?.status;
      if (sid && st === "connected") {
        void api.sshWrite(sid, strToB64(`cd ${shellQuote(dir)}\n`));
        return;
      }
      if (attempt < 40) {
        window.setTimeout(() => sendCd(attempt + 1), 250);
      }
    };
    sendCd(0);
  };

  const downloadRemoteToLocal = async (remotePaths?: string[]) => {
    const sid = sessionRef.current;
    const paths = remotePaths ?? [...selection];
    if (!sid || paths.length === 0 || !localPath) return;
    try {
      setError(null);
      await api.sftpTransferStart(
        sid,
        "download",
        paths,
        localPath,
        useTransferStore.getState().concurrency,
      );
      useTransferStore.getState().setOpen(true);
      // Refresh local pane after a short delay so finished files appear.
      window.setTimeout(() => void loadLocalDir(localPath), 1500);
    } catch (e) {
      setError(String(e));
    }
  };

  /** Upload files that exist only on local (or size-diff) into remote dir. */
  const syncLocalMissing = async () => {
    if (!compareMap || !sessionRef.current) return;
    const paths = localEntries
      .filter((e) => {
        if (e.is_dir) return false;
        const m = compareMap.get(e.name);
        return m === "only-local" || m === "diff";
      })
      .map((e) => e.path);
    if (paths.length === 0) {
      setError("Nothing to upload — remote already has matching files.");
      return;
    }
    const ok = await dialog.confirm({
      title: "Upload to remote?",
      message: `Upload ${paths.length} file(s) from local into ${path}? Size-differing files will be overwritten on the remote.`,
      confirmText: "Upload",
    });
    if (!ok) return;
    try {
      setError(null);
      await useTransferStore.getState().upload(sessionRef.current, path, paths);
    } catch (e) {
      setError(String(e));
    }
  };

  /** Download files that exist only on remote (or size-diff) into local dir. */
  const syncRemoteMissing = async () => {
    if (!compareMap || !sessionRef.current || !localPath) return;
    const paths = entries
      .filter((e) => {
        if (e.is_dir) return false;
        const m = compareMap.get(e.name);
        return m === "only-remote" || m === "diff";
      })
      .map((e) => e.path);
    if (paths.length === 0) {
      setError("Nothing to download — local already has matching files.");
      return;
    }
    const ok = await dialog.confirm({
      title: "Download to local?",
      message: `Download ${paths.length} file(s) from remote into ${localPath}? Size-differing files will be overwritten locally.`,
      confirmText: "Download",
    });
    if (!ok) return;
    void downloadRemoteToLocal(paths);
  };

  // Dual-pane drag: remote → local (download) and local → remote (upload).
  useEffect(() => {
    if (!dualPane) return;
    const offLocal = onInternalDrop(localDropId, (payload) => {
      if (payload.kind !== "remote-file" || payload.vpsId !== data.vpsId) return;
      const paths =
        payload.paths && payload.paths.length > 0
          ? payload.paths
          : payload.path
            ? [payload.path]
            : [];
      if (paths.length === 0) return;
      void downloadRemoteToLocal(paths);
    });
    const offRemote = onInternalDrop(dropId, (payload) => {
      if (payload.kind !== "local-file") return;
      const sid = sessionRef.current;
      if (!sid) return;
      const paths =
        payload.paths && payload.paths.length > 0
          ? payload.paths
          : payload.path
            ? [payload.path]
            : [];
      if (paths.length === 0) return;
      void useTransferStore
        .getState()
        .upload(sid, pathRef.current, paths)
        .then(() => refreshListing())
        .catch((e) => setError(String(e)));
    });
    return () => {
      offLocal();
      offRemote();
    };
    // downloadRemoteToLocal / refreshListing close over current dirs — rebind on nav.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional path deps
  }, [dualPane, localDropId, dropId, data.vpsId, localPath, path]);

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

  // ---------------------------------------------------------------------------
  // Selection
  // ---------------------------------------------------------------------------

  /** What the rows currently show — the directory, or the hits from a search. */
  const visible = useCallback((): SftpEntry[] => {
    if (results) {
      // Search hits are bare paths; give them just enough shape to render and act on.
      return results.map((hit) => ({
        name: hit.slice(hit.lastIndexOf("/") + 1) || hit,
        path: hit,
        is_dir: false,
        size: 0,
        is_symlink: false,
        link_target: null,
        link_broken: false,
      }));
    }
    if (!searchOpen || !query.trim()) return entries;
    const q = query.trim().toLowerCase();
    return entries.filter((e) => e.name.toLowerCase().includes(q));
  }, [results, searchOpen, query, entries]);

  const rows = visible();

  /**
   * Click, ctrl-click and shift-click, the way every file manager does it.
   *
   * Plain click replaces the selection rather than opening — opening moved to
   * double-click, which is what makes a selection possible at all: you cannot select
   * something that navigates away the moment you touch it.
   */
  const clickRow = (entry: SftpEntry, e: React.MouseEvent) => {
    const list = rows.map((r) => r.path);
    if (e.shiftKey) {
      const range = rangeBetween(list, anchorRef.current, entry.path);
      if (range.length > 0) {
        setSelection(new Set(range));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      setSelection((prev) => {
        const next = new Set(prev);
        if (!next.delete(entry.path)) next.add(entry.path);
        return next;
      });
      anchorRef.current = entry.path;
      return;
    }
    setSelection(new Set([entry.path]));
    anchorRef.current = entry.path;
  };

  /**
   * The paths an action applies to.
   *
   * Right-clicking a row that is not in the selection acts on that row alone, which is
   * what the click visibly did; right-clicking inside the selection acts on all of it.
   * Getting this backwards is how a file manager deletes the wrong things.
   */
  const targets = (entry: SftpEntry | null): string[] => actionTargets(entry, selection);

  const clearSelection = () => {
    setSelection(new Set());
    anchorRef.current = null;
  };

  // ---------------------------------------------------------------------------
  // Bulk actions
  // ---------------------------------------------------------------------------

  const bulkDownload = async (entry: SftpEntry | null) => {
    const sid = sessionRef.current;
    const paths = targets(entry);
    if (!sid || paths.length === 0) return;
    try {
      await useTransferStore.getState().download(sid, paths);
    } catch (e) {
      setError(String(e));
    }
  };

  const bulkDelete = async (entry: SftpEntry | null) => {
    const paths = targets(entry);
    if (paths.length === 0) return;
    const listed = paths.slice(0, 8).join("\n");
    const more = paths.length > 8 ? "\n... and " + (paths.length - 8) + " more" : "";
    const what = paths.length === 1 ? paths[0] : paths.length + " items:\n\n" + listed + more;
    if (
      !(await dialog.confirm({
        title: paths.length === 1 ? "Delete" : "Delete " + paths.length + " items",
        message: "Delete " + what + "\n\nEmpty directories only via SFTP; recursive deletes use the remote shell.",
        danger: true,
        confirmText: "Delete",
      }))
    )
      return;
    try {
      const sid = sessionRef.current;
      if (sid && paths.length === 1 && entry) {
        // Native SFTP remove for a single empty dir / file (faster, no shell).
        await api.sftpRemove(sid, entry.path, entry.is_dir);
      } else {
        await api.vpsFileDeleteMany(data.vpsId, paths);
      }
      clearSelection();
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  /** Copy or cut: remember the paths and let the paste decide what to do with them. */
  const putOnClipboard = (entry: SftpEntry | null, mode: "copy" | "cut") => {
    const paths = targets(entry);
    if (paths.length === 0) return;
    fileClipboard = { mode, paths };
    setClipboardTick((n) => n + 1);
  };

  const paste = async () => {
    if (!fileClipboard || fileClipboard.paths.length === 0) return;
    const { mode, paths } = fileClipboard;
    try {
      await api.vpsFileCopy(data.vpsId, paths, path, mode === "cut");
      // A cut is consumed by its paste; a copy stays, so it can be pasted again.
      if (mode === "cut") {
        fileClipboard = null;
        setClipboardTick((n) => n + 1);
      }
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  // ---------------------------------------------------------------------------
  // Search
  // ---------------------------------------------------------------------------

  const closeSearch = () => {
    setSearchOpen(false);
    setAdvanced(false);
    setQuery("");
    setResults(null);
  };

  /** Run the recursive / by-extension search on the server. */
  const runSearch = async () => {
    const exts = parseExtensions(extInput);
    if (!query.trim() && exts.length === 0) {
      setResults(null);
      return;
    }
    setSearching(true);
    try {
      const hits = await api.vpsFileSearch(data.vpsId, path, query.trim(), exts, recursive);
      setResults(hits);
      clearSelection();
    } catch (err) {
      setError(String(err));
    } finally {
      setSearching(false);
    }
  };

  /**
   * Typeahead: start typing anywhere in the panel and the search box opens holding what
   * was already typed.
   *
   * Every printable key appends to `query` whether or not the box is open yet, and the box
   * is a controlled input reading that same state. So the characters that arrive in the
   * window between the box opening and the browser moving focus into it are not lost —
   * that gap is exactly where a naive "open it, then let the input take over" drops a
   * letter or two, and dropping the first letters of a filename makes the whole feature
   * useless.
   */
  const onPanelKeyDown = (e: React.KeyboardEvent) => {
    const el = e.target as HTMLElement;
    if (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable) return;

    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (mod && key === "a") {
      e.preventDefault();
      if (dualPane && activeSide === "local") {
        setLocalSelection(new Set(localEntries.map((r) => r.path)));
      } else {
        setSelection(new Set(rows.map((r) => r.path)));
      }
      return;
    }
    if (mod && key === "c") {
      putOnClipboard(null, "copy");
      return;
    }
    if (mod && key === "x") {
      putOnClipboard(null, "cut");
      return;
    }
    if (mod && key === "v") {
      void paste();
      return;
    }
    if (mod && key === "f") {
      e.preventDefault();
      setSearchOpen(true);
      return;
    }
    if (e.key === "Escape") {
      if (searchOpen) closeSearch();
      else clearSelection();
      return;
    }
    if (e.key === "Delete" && selection.size > 0) {
      e.preventDefault();
      void bulkDelete(null);
      return;
    }
    // WinSCP-style dual-pane transfer keys (work when dual pane is open).
    if (dualPane && e.key === "F5") {
      e.preventDefault();
      if (selection.size > 0) void downloadRemoteToLocal();
      return;
    }
    if (dualPane && e.key === "F6") {
      e.preventDefault();
      if (localSelection.size > 0) void uploadLocalSelection();
      return;
    }
    if (dualPane && e.key === "F7") {
      e.preventDefault();
      setCompareOn((v) => !v);
      return;
    }
    if (e.key === "F2") {
      e.preventDefault();
      const first = [...selection][0];
      if (first) {
        const entry = rows.find((r) => r.path === first);
        if (entry) void handleRename(entry);
      }
      return;
    }
    if (e.key === "Enter" && selection.size === 1) {
      e.preventDefault();
      const first = [...selection][0];
      const entry = rows.find((r) => r.path === first);
      if (entry) openEntry(entry);
      return;
    }
    if (e.key.length !== 1 || mod || e.altKey) return;
    e.preventDefault();
    setQuery((q) => {
      const next = q + e.key;
      if (next.length >= TYPEAHEAD_OPEN_AT) setSearchOpen(true);
      return next;
    });
    if (typeaheadTimer.current) clearTimeout(typeaheadTimer.current);
    typeaheadTimer.current = window.setTimeout(() => {
      // The buffer only expires while the box is still closed. Once it is open the text
      // belongs to the user, not to a typeahead timing out underneath them.
      setSearchOpen((open) => {
        if (!open) setQuery("");
        return open;
      });
    }, TYPEAHEAD_RESET_MS);
  };

  // Focus the box as soon as it opens, so the rest of the word is typed normally into it.
  // `query` already carries whatever arrived before this ran.
  useEffect(() => {
    if (searchOpen) searchInputRef.current?.focus();
  }, [searchOpen]);

  // ---------------------------------------------------------------------------
  // Files dragged in from the desktop
  // ---------------------------------------------------------------------------

  useEffect(() => {
    const offDrop = onOsFilesDropped((target, paths) => {
      if (target !== dropId || paths.length === 0) return;
      const sid = sessionRef.current;
      if (!sid) return;
      void useTransferStore
        .getState()
        .upload(sid, pathRef.current, paths)
        .then(() => refreshListing())
        .catch((e) => setError(String(e)));
    });
    const offHover = onOsDropHover((t) => setDropActive(t === dropId));
    return () => {
      offDrop();
      offHover();
    };
    // `refreshListing` changes with the directory, so this re-subscribes on navigation.
    // That is deliberate: the alternative is a stale closure uploading into the folder
    // that was open when the panel mounted.
  }, [dropId, refreshListing]);

  const openEntry = (entry: SftpEntry) => {
    // A search hit is a path from somewhere else in the tree, and `find` does not say
    // whether it is a file or a directory. Going to its folder and filtering to its name
    // lands on it either way, and shows it in context — which is what someone who
    // searched for it actually wants to see.
    if (results) {
      setResults(null);
      setQuery(entry.name);
      setSearchOpen(true);
      navigateTo(parentDirOf(entry.path));
      return;
    }
    if (entry.is_dir) {
      navigateTo(entry.path);
      return;
    }
    setEditEntry(entry);
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
      const sid = sessionRef.current;
      if (sid) {
        await api.sftpRename(sid, entry.path, to);
      } else {
        await api.vpsFileRename(data.vpsId, entry.path, to);
      }
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  /** Apply octal mode to selection (or the right-clicked entry). */
  const handleChmodSelection = async (entry: SftpEntry) => {
    const paths =
      selection.size > 0 && selection.has(entry.path)
        ? [...selection]
        : [entry.path];
    const mode = await dialog.prompt({
      title: paths.length > 1 ? `Chmod ${paths.length} items` : "Chmod",
      label: "Octal mode (e.g. 755 or 644)",
      defaultValue: "644",
      confirmText: "Apply",
    });
    if (!mode?.trim()) return;
    const cleaned = mode.trim().replace(/^0/, "");
    if (!/^[0-7]{3,4}$/.test(cleaned)) {
      setError("Invalid mode — use 3–4 octal digits (e.g. 755).");
      return;
    }
    try {
      setError(null);
      for (const p of paths) {
        await api.vpsFileChmod(data.vpsId, p, cleaned, false);
      }
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
      const dest = joinRemotePath(path, name.trim());
      const sid = sessionRef.current;
      if (sid) {
        await api.sftpMkdir(sid, dest);
      } else {
        await api.vpsFileMkdir(data.vpsId, dest);
      }
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

  /// Repoint an existing symlink. Pre-filled with the current target, because the usual
  /// edit is a small change to it — a version number — not typing a path from scratch.
  const handleEditLink = async (entry: SftpEntry) => {
    const next = await dialog.prompt({
      title: `Link target for ${entry.name}`,
      label: "Points to (relative paths are kept relative)",
      defaultValue: entry.link_target ?? "",
      confirmText: "Save",
    });
    if (next === null || !next.trim()) return;
    try {
      // Replace: remove old link then create new (SFTP has no atomic retarget).
      const sid = sessionRef.current;
      if (sid) {
        await api.sftpRemove(sid, entry.path, false).catch(() => {});
        await api.sftpSymlink(sid, entry.path, next.trim());
      } else {
        await api.vpsFileSymlink(data.vpsId, entry.path, next.trim());
      }
      refreshListing();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleNewLink = async () => {
    const name = await dialog.prompt({
      title: "New symlink",
      label: "Link name",
      confirmText: "Next",
    });
    if (!name?.trim()) return;
    const target = await dialog.prompt({
      title: `Link target for ${name.trim()}`,
      label: "Points to (relative paths are kept relative)",
      confirmText: "Create",
    });
    if (!target?.trim()) return;
    try {
      const linkPath = joinRemotePath(path, name.trim());
      const sid = sessionRef.current;
      if (sid) {
        await api.sftpSymlink(sid, linkPath, target.trim());
      } else {
        await api.vpsFileSymlink(data.vpsId, linkPath, target.trim());
      }
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
  // Read through the tick so the menu re-renders when another panel fills the
  // clipboard — the clipboard itself is module state and cannot be subscribed to.
  const canPaste = clipboardTick >= 0 && !!fileClipboard && fileClipboard.paths.length > 0;

  const freeform = layoutMode === "freeform";
  const tiled = layoutMode === "tile";
  const zoom = useStore((s) => s.transform[2]);

  return (
    <div
      ref={panelRef}
      className={`group flex h-full w-full flex-col overflow-hidden border bg-[var(--bg)] shadow-lg ${
        tiled ? "rounded-none" : "rounded-lg"
      } ${selected ? "border-cyan-500" : "border-[var(--border)]"}`}
      onMouseDown={() => focus(id)}
      // Focusable so the panel receives keys at all; the outline is suppressed because
      // the whole panel lighting up on every click is noise, and the selected rows
      // already show where the keyboard is pointed.
      tabIndex={0}
      onKeyDown={onPanelKeyDown}
      style={{
        ...(freeform ? {} : { transform: `scale(${1 / zoom})`, transformOrigin: "top left" }),
        outline: "none",
      }}
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
          <GitBranchBadge info={remoteGit} className="ml-1" />
          <button
            type="button"
            className={`rounded px-1.5 py-0.5 text-[10px] ${
              dualPane
                ? "bg-cyan-900/40 text-cyan-200"
                : "text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            }`}
            data-tooltip={
              dualPane ? "Hide local pane" : "Dual pane: local PC | remote (WinSCP-style)"
            }
            onClick={() => setDualPane((v) => !v)}
          >
            ⧉ Dual
          </button>
          <button
            type="button"
            className={`rounded px-1.5 py-0.5 text-[10px] ${
              bookmarks.includes(path)
                ? "text-amber-300"
                : "text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
            }`}
            data-tooltip={
              bookmarks.includes(path) ? "Remove bookmark" : "Bookmark this path"
            }
            onClick={toggleBookmark}
          >
            ★
          </button>
          {bookmarks.length > 0 ? (
            <select
              className="max-w-[140px] rounded border border-[var(--border)] bg-[var(--bg)] px-1 py-0.5 text-[10px] text-[var(--text-dim)]"
              defaultValue=""
              data-tooltip="Bookmarks"
              onChange={(e) => {
                if (e.target.value) void openDir(e.target.value);
                e.target.value = "";
              }}
            >
              <option value="" disabled>
                ★
              </option>
              {bookmarks.map((b) => (
                <option key={b} value={b}>
                  {b}
                </option>
              ))}
            </select>
          ) : null}
          <button
            type="button"
            className={`rounded px-1.5 py-0.5 text-[10px] ${
              searchOpen
                ? "bg-[var(--border)] text-gray-200"
                : "text-gray-400 hover:bg-[var(--border)]"
            }`}
            data-tooltip="Find in this folder (Ctrl+F, or just start typing)"
            onClick={() => (searchOpen ? closeSearch() : setSearchOpen(true))}
          >
            Find
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

        {/* Clickable breadcrumb — faster than retyping the path. */}
        <div className="flex min-w-0 shrink-0 items-center gap-0.5 overflow-x-auto border-b border-[var(--border)]/60 px-2 py-0.5">
          <button
            type="button"
            className="shrink-0 rounded px-1 py-0.5 font-mono text-[10px] text-cyan-400/90 hover:bg-[var(--border)] hover:text-cyan-200"
            onClick={() => navigateTo("/")}
            data-tooltip="Go to /"
          >
            /
          </button>
          {pathSegments(path).map((seg, i, all) => {
            const full = `/${all.slice(0, i + 1).join("/")}`;
            const last = i === all.length - 1;
            return (
              <span key={full} className="flex shrink-0 items-center gap-0.5">
                <span className="text-[10px] text-gray-600">/</span>
                <button
                  type="button"
                  disabled={last}
                  className={`max-w-[120px] truncate rounded px-1 py-0.5 font-mono text-[10px] ${
                    last
                      ? "text-gray-200"
                      : "text-cyan-400/90 hover:bg-[var(--border)] hover:text-cyan-200"
                  } disabled:cursor-default`}
                  onClick={() => {
                    if (!last) navigateTo(full);
                  }}
                  data-tooltip={full}
                >
                  {seg}
                </button>
              </span>
            );
          })}
        </div>

        {searchOpen && (
          <div className="border-b border-[var(--border)] bg-[var(--surface)]/60 px-2 py-1">
            <div className="flex items-center gap-1.5">
              <span className="shrink-0 text-[10px] text-gray-500">Find</span>
              <input
                ref={searchInputRef}
                type="text"
                className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-200 outline-none focus:border-cyan-600"
                value={query}
                spellCheck={false}
                placeholder="name contains…"
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    closeSearch();
                    panelRef.current?.focus();
                  }
                  if (e.key === "Enter") {
                    // In the directory, Enter opens the single match — the fastest path
                    // from "type a name" to "be in it". In advanced mode it searches.
                    if (advanced || recursive) void runSearch();
                    else if (rows.length > 0) openEntry(rows[0]);
                  }
                }}
              />
              <button
                type="button"
                className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] ${
                  advanced
                    ? "bg-[var(--border)] text-gray-200"
                    : "text-gray-400 hover:bg-[var(--border)]"
                }`}
                data-tooltip="Search subdirectories, filter by extension"
                onClick={() => setAdvanced((v) => !v)}
              >
                Advanced
              </button>
              <button
                type="button"
                className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
                onClick={() => {
                  closeSearch();
                  panelRef.current?.focus();
                }}
              >
                ✕
              </button>
            </div>

            {advanced && (
              <div className="mt-1 flex flex-wrap items-center gap-2">
                <input
                  type="text"
                  className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 font-mono text-[10px] text-gray-300 outline-none focus:border-cyan-600"
                  value={extInput}
                  spellCheck={false}
                  placeholder="extensions: php, js, tar.gz"
                  onChange={(e) => setExtInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void runSearch();
                  }}
                />
                <label className="flex shrink-0 cursor-pointer items-center gap-1 text-[10px] text-gray-400">
                  <input
                    type="checkbox"
                    checked={recursive}
                    onChange={(e) => setRecursive(e.target.checked)}
                  />
                  All subdirectories
                </label>
                <button
                  type="button"
                  className="shrink-0 rounded border border-cyan-800/60 px-2 py-0.5 text-[10px] text-cyan-300 hover:bg-cyan-900/30 disabled:opacity-40"
                  disabled={searching}
                  onClick={() => void runSearch()}
                >
                  {searching ? "Searching…" : "Search"}
                </button>
                {results && (
                  <button
                    type="button"
                    className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-gray-400 hover:bg-[var(--border)]"
                    onClick={() => setResults(null)}
                  >
                    Back to folder
                  </button>
                )}
              </div>
            )}

            {/* Say what is on screen. A filtered list and a short directory look
                identical, and a search that found nothing looks like an empty folder. */}
            <div className="mt-0.5 text-[10px] text-gray-600">
              {results
                ? `${results.length} match${results.length === 1 ? "" : "es"}${
                    results.length >= 500 ? " (capped)" : ""
                  } under ${path}`
                : query.trim()
                  ? `${rows.length} of ${entries.length} in this folder`
                  : "Type to filter this folder — Advanced searches inside subdirectories"}
            </div>
          </div>
        )}

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

        {selection.size > 1 && (
          <div className="flex items-center gap-2 border-b border-cyan-900/40 bg-cyan-950/30 px-2 py-1 text-[10px] text-cyan-200">
            <span className="shrink-0">{selection.size} selected</span>
            <button
              type="button"
              className="rounded px-1.5 py-0.5 hover:bg-cyan-900/40"
              onClick={() => void bulkDownload(null)}
            >
              Download
            </button>
            <button
              type="button"
              className="rounded px-1.5 py-0.5 hover:bg-cyan-900/40"
              onClick={() => putOnClipboard(null, "copy")}
            >
              Copy
            </button>
            <button
              type="button"
              className="rounded px-1.5 py-0.5 hover:bg-cyan-900/40"
              onClick={() => putOnClipboard(null, "cut")}
            >
              Cut
            </button>
            <button
              type="button"
              className="rounded px-1.5 py-0.5 text-red-300 hover:bg-red-900/40"
              onClick={() => void bulkDelete(null)}
            >
              Delete
            </button>
            {canPaste && (
              <button
                type="button"
                className="rounded px-1.5 py-0.5 hover:bg-cyan-900/40"
                onClick={() => void paste()}
              >
                Paste here
              </button>
            )}
            <button
              type="button"
              className="ml-auto shrink-0 rounded px-1.5 py-0.5 text-gray-400 hover:bg-[var(--border)]"
              onClick={clearSelection}
            >
              Clear
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
            {dualPane && (
              <div
                className={`relative flex min-h-0 w-[42%] min-w-[160px] max-w-[50%] shrink-0 flex-col border-r border-[var(--border)] ${
                  activeSide === "local" ? "bg-cyan-950/10 ring-1 ring-inset ring-cyan-800/30" : ""
                }`}
                onMouseDown={() => setActiveSide("local")}
              >
                <div className="flex items-center gap-1 border-b border-[var(--border)]/80 px-1.5 py-1">
                  <button
                    type="button"
                    className="rounded px-1 text-[10px] text-gray-400 hover:bg-[var(--border)] disabled:opacity-30"
                    disabled={!localPath || localLoading}
                    onClick={() => {
                      const parent = localPath.replace(/[\\/][^\\/]+$/, "") || localPath;
                      void loadLocalDir(parent);
                    }}
                    data-tooltip="Up"
                  >
                    ‹
                  </button>
                  <span
                    className="min-w-0 flex-1 truncate font-mono text-[10px] text-gray-400"
                    title={localPath}
                  >
                    {localPath || "Local"}
                  </span>
                  <GitBranchBadge info={localGit} />
                  <button
                    type="button"
                    className="rounded px-1.5 py-0.5 text-[10px] text-cyan-300 hover:bg-cyan-900/30 disabled:opacity-40"
                    disabled={selection.size === 0 || status !== "connected" || !localPath}
                    onClick={() => void downloadRemoteToLocal()}
                    data-tooltip="Download selected remote items into this local folder"
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    className="rounded px-1.5 py-0.5 text-[10px] text-cyan-300 hover:bg-cyan-900/30 disabled:opacity-40"
                    disabled={localSelection.size === 0 || status !== "connected"}
                    onClick={() => void uploadLocalSelection()}
                    data-tooltip="Upload selected local files/folders to the remote folder"
                  >
                    ↑ Upload
                  </button>
                  <button
                    type="button"
                    className={`rounded px-1.5 py-0.5 text-[10px] ${
                      compareOn
                        ? "bg-amber-900/40 text-amber-200"
                        : "text-gray-400 hover:bg-[var(--border)] hover:text-gray-200"
                    }`}
                    onClick={() => setCompareOn((v) => !v)}
                    data-tooltip="Compare local vs remote files by name and size"
                  >
                    ⇄
                  </button>
                  {compareOn && compareStats ? (
                    <>
                      <span
                        className="max-w-[90px] truncate font-mono text-[9px] text-gray-500"
                        title={`Only local: ${compareStats.onlyLocal} · Only remote: ${compareStats.onlyRemote} · Size diff: ${compareStats.diff}`}
                      >
                        L{compareStats.onlyLocal} R{compareStats.onlyRemote} Δ
                        {compareStats.diff}
                      </span>
                      <button
                        type="button"
                        className="rounded px-1 py-0.5 text-[9px] text-emerald-300 hover:bg-emerald-950/40 disabled:opacity-40"
                        disabled={
                          compareStats.onlyLocal + compareStats.diff === 0 ||
                          status !== "connected"
                        }
                        onClick={() => void syncLocalMissing()}
                        data-tooltip="Upload only-local + size-diff files to remote"
                      >
                        ↑ miss
                      </button>
                      <button
                        type="button"
                        className="rounded px-1 py-0.5 text-[9px] text-emerald-300 hover:bg-emerald-950/40 disabled:opacity-40"
                        disabled={
                          compareStats.onlyRemote + compareStats.diff === 0 ||
                          status !== "connected" ||
                          !localPath
                        }
                        onClick={() => void syncRemoteMissing()}
                        data-tooltip="Download only-remote + size-diff files to local"
                      >
                        ↓ miss
                      </button>
                    </>
                  ) : null}
                </div>
                <div
                  className="relative min-h-0 flex-1 overflow-y-auto px-1 py-1"
                  data-drop={localDropId}
                >
                  {localDropHighlight && (
                    <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center border-2 border-dashed border-emerald-500 bg-emerald-500/10 text-[11px] text-emerald-200">
                      Download to {localPath || "local"}
                    </div>
                  )}
                  {localLoading && localEntries.length === 0 ? (
                    <div className="px-2 py-3 text-center text-[10px] text-gray-500">
                      Loading…
                    </div>
                  ) : localEntries.length === 0 ? (
                    <div className="px-2 py-3 text-center text-[10px] text-gray-600">
                      Empty folder — drag remote files here to download
                    </div>
                  ) : (
                    localEntries.map((entry) => {
                      const mark =
                        !entry.is_dir && compareMap
                          ? compareMap.get(entry.name)
                          : undefined;
                      const markCls =
                        mark === "only-local"
                          ? "bg-emerald-950/50 ring-1 ring-inset ring-emerald-700/40"
                          : mark === "diff"
                            ? "bg-amber-950/45 ring-1 ring-inset ring-amber-700/40"
                            : localSelection.has(entry.path)
                              ? "bg-cyan-950/60 ring-1 ring-inset ring-cyan-700/50"
                              : "hover:bg-[var(--surface)]";
                      return (
                      <div
                        key={entry.path}
                        className={`flex items-center gap-1.5 rounded px-1.5 py-0.5 text-[11px] ${markCls}`}
                        title={
                          mark === "only-local"
                            ? "Only on local"
                            : mark === "diff"
                              ? "Size differs from remote"
                              : mark === "same"
                                ? "Same name+size as remote"
                                : undefined
                        }
                      >
                        <button
                          type="button"
                          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                          onClick={(e) => {
                            setLocalSelection((prev) => {
                              const next = new Set(prev);
                              if (e.ctrlKey || e.metaKey) {
                                if (next.has(entry.path)) next.delete(entry.path);
                                else next.add(entry.path);
                              } else {
                                return new Set([entry.path]);
                              }
                              return next;
                            });
                          }}
                          onDoubleClick={() => {
                            if (entry.is_dir) void loadLocalDir(entry.path);
                          }}
                          onPointerDown={(e) => {
                            if (e.button !== 0) return;
                            const paths =
                              localSelection.has(entry.path) && localSelection.size > 1
                                ? [...localSelection]
                                : [entry.path];
                            startInternalDrag(e, {
                              kind: "local-file",
                              vpsId: data.vpsId,
                              path: entry.path,
                              paths,
                              label:
                                paths.length > 1
                                  ? `${paths.length} local items`
                                  : entry.name,
                              isDir: entry.is_dir,
                            });
                          }}
                        >
                          <span className="shrink-0 text-gray-500">
                            {entry.is_dir ? "📁" : "📄"}
                          </span>
                          <span className="min-w-0 truncate text-gray-200">{entry.name}</span>
                          {!entry.is_dir && (
                            <span className="ml-auto shrink-0 tabular-nums text-[10px] text-gray-600">
                              {entry.size < 1024
                                ? `${entry.size} B`
                                : entry.size < 1024 * 1024
                                  ? `${(entry.size / 1024).toFixed(1)} KB`
                                  : `${(entry.size / (1024 * 1024)).toFixed(1)} MB`}
                            </span>
                          )}
                          {mark === "only-local" ? (
                            <span className="shrink-0 text-[9px] text-emerald-400">L</span>
                          ) : mark === "diff" ? (
                            <span className="shrink-0 text-[9px] text-amber-400">Δ</span>
                          ) : null}
                        </button>
                      </div>
                    );
                    })
                  )}
                </div>
              </div>
            )}

            {showTree && !dualPane && (
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
              className={`relative min-h-0 flex-1 overflow-y-auto px-1 py-1 ${
                dualPane && activeSide === "remote"
                  ? "bg-cyan-950/10 ring-1 ring-inset ring-cyan-800/30"
                  : ""
              }`}
              data-drop={dropId}
              onMouseDown={() => setActiveSide("remote")}
              onContextMenu={(e) => showContextMenu(e, null)}
              onClick={(e) => {
                // A click on the empty space below the rows clears the selection, the
                // same as it does on a desktop.
                if (e.target === e.currentTarget) clearSelection();
              }}
            >
              {remoteDropHighlight && (
                <div className="pointer-events-none absolute inset-0 z-20 flex items-center justify-center border-2 border-dashed border-cyan-500 bg-cyan-500/10 text-xs text-cyan-200">
                  Upload to {path}
                </div>
              )}
              {loading && rows.length === 0 ? (
                <div className="px-2 py-4 text-center text-xs text-gray-500">Loading…</div>
              ) : rows.length === 0 ? (
                <div className="px-2 py-4 text-center text-xs text-gray-600">
                  {results
                    ? "Nothing matched"
                    : query.trim()
                      ? "Nothing in this folder matches"
                      : "Empty directory"}
                </div>
              ) : (
                rows.map((entry) => {
                  const mark =
                    !entry.is_dir && compareMap
                      ? compareMap.get(entry.name)
                      : undefined;
                  const rowCls =
                    mark === "only-remote"
                      ? "bg-sky-950/45 ring-1 ring-inset ring-sky-700/40"
                      : mark === "diff"
                        ? "bg-amber-950/45 ring-1 ring-inset ring-amber-700/40"
                        : selection.has(entry.path)
                          ? "bg-cyan-950/60 ring-1 ring-inset ring-cyan-700/60"
                          : "hover:bg-[var(--surface)]";
                  return (
                  <div
                    key={entry.path}
                    className={`group flex items-center gap-2 rounded px-2 py-1 ${rowCls}`}
                    title={
                      mark === "only-remote"
                        ? "Only on remote"
                        : mark === "diff"
                          ? "Size differs from local"
                          : mark === "same"
                            ? "Same name+size as local"
                            : undefined
                    }
                    onContextMenu={(e) => {
                      // Right-clicking outside the selection moves the selection there
                      // first, so the menu always acts on what is visibly highlighted.
                      if (!selection.has(entry.path)) setSelection(new Set([entry.path]));
                      showContextMenu(e, entry);
                    }}
                  >
                    <button
                      type="button"
                      className="flex min-w-0 flex-1 items-center gap-2 text-left"
                      onClick={(e) => clickRow(entry, e)}
                      onDoubleClick={() => openEntry(entry)}
                      // Drag onto a terminal to type this path there. Pointer-based,
                      // and only arms after a few pixels of movement, so the click
                      // above still opens the entry.
                      onPointerDown={(e) => {
                        if (e.button !== 0) return;
                        const paths =
                          selection.has(entry.path) && selection.size > 1
                            ? [...selection]
                            : [entry.path];
                        startInternalDrag(e, {
                          kind: "remote-file",
                          vpsId: data.vpsId,
                          path: entry.path,
                          paths,
                          label:
                            paths.length > 1
                              ? `${paths.length} remote items`
                              : entry.name,
                          isDir: entry.is_dir,
                        });
                      }}
                    >
                      {(() => {
                        // Shape says what family it belongs to, colour says which member —
                        // see fileIcons.tsx for why both are needed at this size.
                        const kind = fileKindFor(entry);
                        return (
                          <span
                            className={`shrink-0 ${kind.className}`}
                            data-tooltip={
                              entry.link_broken
                                ? "Broken symlink — its target does not exist"
                                : kind.label
                            }
                          >
                            <kind.Icon size={17} />
                          </span>
                        );
                      })()}
                      <span
                        className={`truncate text-xs ${
                          entry.link_broken ? "text-red-300/80" : "text-gray-200"
                        }`}
                      >
                        {entry.name}
                      </span>
                      {/* Where a hit lives. Twenty files called index.php are the normal
                          outcome of a recursive search, and the name alone cannot tell
                          them apart. */}
                      {results && (
                        <span className="truncate font-mono text-[10px] text-gray-600">
                          {parentDirOf(entry.path)}
                        </span>
                      )}
                      {/* The target inline: a link is only meaningful together with where
                          it points, and opening a dialog to find out defeats the purpose. */}
                      {entry.link_target && (
                        <span className="truncate font-mono text-[10px] text-violet-300/70">
                          → {entry.link_target}
                        </span>
                      )}
                      {!entry.is_dir && !entry.is_symlink && (
                        <span className="ml-auto shrink-0 font-mono text-[10px] text-gray-600">
                          {formatSize(entry.size)}
                        </span>
                      )}
                      {mark === "only-remote" ? (
                        <span className="shrink-0 text-[9px] text-sky-400">R</span>
                      ) : mark === "diff" ? (
                        <span className="shrink-0 text-[9px] text-amber-400">Δ</span>
                      ) : null}
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
                  );
                })
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
          onChmodSelection={(e) => void handleChmodSelection(e)}
          onRename={(e) => void handleRename(e)}
          onDelete={(e) => void bulkDelete(e)}
          onCopyPath={(p) => void handleCopyPath(p)}
          onOpenTerminalHere={(e) => openTerminalHere(e)}
          onNewFolder={() => void handleNewFolder()}
          onNewFile={() => void handleNewFile()}
          selectionCount={selection.size}
          onDownloadSelection={(e) => void bulkDownload(e)}
          onDeleteSelection={(e) => void bulkDelete(e)}
          onCopy={(e) => putOnClipboard(e, "copy")}
          onCut={(e) => putOnClipboard(e, "cut")}
          onPaste={() => void paste()}
          canPaste={canPaste}
          onEditLink={(e) => void handleEditLink(e)}
          onNewLink={() => void handleNewLink()}
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
