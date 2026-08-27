import { useState, useMemo } from "react";
import { FolderIcon, FileIcon, RefreshIcon } from "./icons";
import type { SftpFileEntry } from "./types";

export function SftpNode({
  vpsId = "vps-local",
  initialPath = "/var/www",
  onClose,
}: {
  vpsId?: string;
  initialPath?: string;
  onClose?: () => void;
}) {
  const [currentPath, setCurrentPath] = useState(initialPath);
  const [pathInput, setPathInput] = useState(initialPath);
  const [isEditingPath, setIsEditingPath] = useState(false);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [selectedFile, setSelectedFile] = useState<SftpFileEntry | null>(null);

  // Modals state
  const [editorFile, setEditorFile] = useState<{ path: string; name: string; content: string } | null>(null);
  const [savingFile, setSavingFile] = useState(false);
  const [chmodFile, setChmodFile] = useState<SftpFileEntry | null>(null);
  const [chmodOctal, setChmodOctal] = useState("0755");
  const [newDialog, setNewDialog] = useState<"file" | "folder" | null>(null);
  const [newItemName, setNewItemName] = useState("");
  const [deleteConfirm, setDeleteConfirm] = useState<SftpFileEntry | null>(null);

  // File list state
  const [files, setFiles] = useState<SftpFileEntry[]>([
    { name: "..", path: "/var", is_dir: true, size: 0, modified: Date.now() - 3600000, permissions: "drwxr-xr-x" },
    { name: "html", path: "/var/www/html", is_dir: true, size: 4096, modified: Date.now() - 7200000, permissions: "drwxr-xr-x" },
    { name: "index.html", path: "/var/www/index.html", is_dir: false, size: 2048, modified: Date.now() - 1800000, permissions: "-rw-r--r--" },
    { name: "package.json", path: "/var/www/package.json", is_dir: false, size: 1024, modified: Date.now() - 900000, permissions: "-rw-r--r--" },
    { name: "server.ts", path: "/var/www/server.ts", is_dir: false, size: 4096, modified: Date.now() - 300000, permissions: "-rw-r--r--" },
    { name: ".env", path: "/var/www/.env", is_dir: false, size: 256, modified: Date.now() - 86400000, permissions: "-rw-------" },
  ]);

  const loadDirectory = (targetPath: string) => {
    setLoading(true);
    setCurrentPath(targetPath);
    setPathInput(targetPath);

    // Try Tauri API if available in runtime
    if (typeof window !== "undefined" && (window as any).__TAURI__) {
      (window as any).__TAURI__.core
        .invoke("sftp_list", { sessionId: vpsId, path: targetPath })
        .then((res: any) => {
          if (res && res.entries) {
            setFiles(res.entries);
          }
        })
        .catch(() => {
          // Keep local state if running standalone
        })
        .finally(() => setLoading(false));
    } else {
      setTimeout(() => setLoading(false), 200);
    }
  };

  const handleOpenEntry = (file: SftpFileEntry) => {
    if (file.is_dir) {
      if (file.name === "..") {
        const parts = currentPath.split("/").filter(Boolean);
        parts.pop();
        const parent = "/" + parts.join("/");
        loadDirectory(parent || "/");
      } else {
        loadDirectory(file.path);
      }
    } else {
      // Open editor
      setEditorFile({
        path: file.path,
        name: file.name,
        content: `// Content of ${file.path}\n// Loaded via xConsole SFTP Plugin\n\nconsole.log("Ready to edit remote file: ${file.name}");\n`,
      });
    }
  };

  const handleSaveFile = () => {
    if (!editorFile) return;
    setSavingFile(true);
    setTimeout(() => {
      setSavingFile(false);
      setEditorFile(null);
    }, 400);
  };

  const handleCreateItem = () => {
    if (!newItemName.trim() || !newDialog) return;
    const isDir = newDialog === "folder";
    const fullPath = `${currentPath.replace(/\/$/, "")}/${newItemName.trim()}`;
    const newEntry: SftpFileEntry = {
      name: newItemName.trim(),
      path: fullPath,
      is_dir: isDir,
      size: isDir ? 4096 : 0,
      modified: Date.now(),
      permissions: isDir ? "drwxr-xr-x" : "-rw-r--r--",
    };
    setFiles((prev) => [...prev, newEntry]);
    setNewDialog(null);
    setNewItemName("");
  };

  const handleDeleteItem = (target: SftpFileEntry) => {
    setFiles((prev) => prev.filter((f) => f.path !== target.path));
    setDeleteConfirm(null);
  };

  const filteredFiles = useMemo(() => {
    if (!search.trim()) return files;
    const q = search.toLowerCase();
    return files.filter((f) => f.name.toLowerCase().includes(q));
  }, [files, search]);

  const breadcrumbs = useMemo(() => {
    const parts = currentPath.split("/").filter(Boolean);
    return [
      { name: "root (/)", path: "/" },
      ...parts.map((p, i) => ({
        name: p,
        path: "/" + parts.slice(0, i + 1).join("/"),
      })),
    ];
  }, [currentPath]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-2xl font-mono text-xs select-none">
      {/* Top Header Bar */}
      <div className="flex h-9 items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-3 shrink-0">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <FolderIcon size={14} className="text-amber-400 shrink-0" />
          <span className="font-semibold text-gray-200 truncate font-sans text-xs">
            SFTP File Manager
          </span>
          <span className="rounded bg-zinc-800 text-zinc-400 border border-zinc-700 px-1.5 py-0.2 text-[10px]">
            {vpsId}
          </span>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          <button
            onClick={() => setNewDialog("file")}
            className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700 px-2 py-0.5 text-[10px] font-medium"
          >
            + File
          </button>
          <button
            onClick={() => setNewDialog("folder")}
            className="rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700 px-2 py-0.5 text-[10px] font-medium"
          >
            + Folder
          </button>
          <button
            onClick={() => loadDirectory(currentPath)}
            className="rounded p-1 text-zinc-400 hover:text-white hover:bg-white/5"
            title="Refresh"
          >
            <RefreshIcon size={13} />
          </button>
          {onClose && (
            <button
              onClick={onClose}
              className="rounded p-1 text-zinc-400 hover:text-red-400 hover:bg-white/5 ml-1"
            >
              ✕
            </button>
          )}
        </div>
      </div>

      {/* Path & Breadcrumbs Bar */}
      <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 gap-2 shrink-0">
        <div className="flex items-center gap-1 overflow-x-auto flex-1 min-w-0">
          {isEditingPath ? (
            <input
              value={pathInput}
              onChange={(e) => setPathInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  loadDirectory(pathInput);
                  setIsEditingPath(false);
                } else if (e.key === "Escape") {
                  setIsEditingPath(false);
                }
              }}
              autoFocus
              className="w-full rounded bg-[var(--surface-2)] border border-zinc-500 px-2 py-0.5 text-xs text-white focus:outline-none"
            />
          ) : (
            <div
              className="flex items-center gap-1 text-[11px] cursor-text truncate"
              onClick={() => setIsEditingPath(true)}
            >
              {breadcrumbs.map((b, i) => (
                <span key={b.path} className="flex items-center gap-1">
                  <span
                    onClick={(e) => {
                      e.stopPropagation();
                      loadDirectory(b.path);
                    }}
                    className="text-zinc-400 hover:text-white hover:underline cursor-pointer"
                  >
                    {b.name}
                  </span>
                  {i < breadcrumbs.length - 1 && <span className="text-zinc-600">/</span>}
                </span>
              ))}
            </div>
          )}
        </div>

        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Filter files..."
          className="rounded bg-[var(--surface-2)] border border-[var(--border)] px-2 py-0.5 text-[11px] text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-zinc-400 w-36 shrink-0"
        />
      </div>

      {/* File List Table */}
      <div className="flex-1 overflow-y-auto p-2">
        {loading ? (
          <div className="flex items-center justify-center p-8 text-zinc-500">
            <span className="animate-spin mr-2">⠋</span> Reading remote directory...
          </div>
        ) : (
          <table className="w-full text-left text-xs">
            <thead>
              <tr className="text-zinc-500 border-b border-[var(--border)] text-[10px] uppercase">
                <th className="pb-1.5 pl-2 font-medium">Name</th>
                <th className="pb-1.5 font-medium">Size</th>
                <th className="pb-1.5 font-medium hidden sm:table-cell">Permissions</th>
                <th className="pb-1.5 pr-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--border)]/40">
              {filteredFiles.map((file) => {
                const isSelected = selectedFile?.path === file.path;

                return (
                  <tr
                    key={file.path}
                    onClick={() => setSelectedFile(file)}
                    onDoubleClick={() => handleOpenEntry(file)}
                    className={`hover:bg-[var(--surface-hover)] cursor-pointer transition ${
                      isSelected ? "bg-zinc-800/60" : ""
                    }`}
                  >
                    <td className="py-1.5 pl-2 flex items-center gap-2">
                      {file.is_dir ? (
                        <FolderIcon size={13} className="text-amber-400 shrink-0" />
                      ) : (
                        <FileIcon size={13} className="text-cyan-400 shrink-0" />
                      )}
                      <span className={`truncate ${file.is_dir ? "font-semibold text-gray-200" : "text-zinc-300"}`}>
                        {file.name}
                      </span>
                    </td>
                    <td className="py-1.5 text-zinc-400 text-[11px]">
                      {file.is_dir ? "-" : `${file.size} B`}
                    </td>
                    <td className="py-1.5 text-zinc-500 text-[11px] font-mono hidden sm:table-cell">
                      {file.permissions || (file.is_dir ? "drwxr-xr-x" : "-rw-r--r--")}
                    </td>
                    <td className="py-1.5 pr-2 text-right">
                      {file.name !== ".." && (
                        <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
                          {!file.is_dir && (
                            <button
                              onClick={() => handleOpenEntry(file)}
                              className="rounded px-1.5 py-0.5 text-[10px] text-zinc-400 hover:text-white hover:bg-white/10"
                              title="Edit"
                            >
                              Edit
                            </button>
                          )}
                          <button
                            onClick={() => {
                              setChmodFile(file);
                              setChmodOctal(file.is_dir ? "0755" : "0644");
                            }}
                            className="rounded px-1.5 py-0.5 text-[10px] text-zinc-400 hover:text-white hover:bg-white/10"
                            title="Permissions"
                          >
                            chmod
                          </button>
                          <button
                            onClick={() => setDeleteConfirm(file)}
                            className="rounded px-1.5 py-0.5 text-[10px] text-red-400 hover:text-red-300 hover:bg-red-950/40"
                            title="Delete"
                          >
                            ✕
                          </button>
                        </div>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Footer Status */}
      <div className="flex items-center justify-between border-t border-[var(--border)] px-3 py-1.5 bg-[var(--surface-2)] text-[10px] text-zinc-500 shrink-0">
        <div>{files.length} items</div>
        <div className="text-zinc-400">Double click to open folder or edit file</div>
      </div>

      {/* Editor Modal */}
      {editorFile && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="flex h-[80vh] w-[min(800px,95vw)] flex-col rounded-xl border border-[var(--border-strong)] bg-[var(--surface)] shadow-2xl overflow-hidden">
            <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3 bg-[var(--surface-2)]">
              <div className="flex items-center gap-2">
                <FileIcon size={14} className="text-cyan-400" />
                <span className="font-semibold text-gray-200">{editorFile.name}</span>
                <span className="text-zinc-500 text-[10px]">({editorFile.path})</span>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={handleSaveFile}
                  disabled={savingFile}
                  className="rounded bg-zinc-100 hover:bg-white text-zinc-950 px-3 py-1 text-xs font-bold transition"
                >
                  {savingFile ? "Saving…" : "Save File"}
                </button>
                <button
                  onClick={() => setEditorFile(null)}
                  className="text-zinc-400 hover:text-white px-2 py-1"
                >
                  ✕
                </button>
              </div>
            </div>
            <textarea
              value={editorFile.content}
              onChange={(e) => setEditorFile({ ...editorFile, content: e.target.value })}
              className="flex-1 resize-none bg-black/60 p-4 font-mono text-xs text-gray-200 outline-none leading-relaxed"
            />
          </div>
        </div>
      )}

      {/* Chmod Dialog */}
      {chmodFile && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="w-80 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 shadow-2xl space-y-3">
            <h4 className="text-sm font-semibold text-gray-200">Change Permissions</h4>
            <p className="text-xs text-zinc-400 truncate">{chmodFile.name}</p>
            <div>
              <label className="text-[10px] text-zinc-500 uppercase">Octal Mode (e.g. 0755, 0644):</label>
              <input
                value={chmodOctal}
                onChange={(e) => setChmodOctal(e.target.value)}
                className="mt-1 w-full rounded border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1 text-xs text-white"
              />
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <button
                onClick={() => setChmodFile(null)}
                className="rounded px-2.5 py-1 text-xs text-zinc-400 hover:text-white"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  setFiles((prev) =>
                    prev.map((f) => (f.path === chmodFile.path ? { ...f, permissions: chmodOctal } : f))
                  );
                  setChmodFile(null);
                }}
                className="rounded bg-zinc-100 px-3 py-1 text-xs font-bold text-zinc-950"
              >
                Apply
              </button>
            </div>
          </div>
        </div>
      )}

      {/* New Item Dialog */}
      {newDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="w-80 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 shadow-2xl space-y-3">
            <h4 className="text-sm font-semibold text-gray-200">
              New {newDialog === "folder" ? "Folder" : "File"}
            </h4>
            <div>
              <label className="text-[10px] text-zinc-500 uppercase">Name:</label>
              <input
                value={newItemName}
                onChange={(e) => setNewItemName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleCreateItem()}
                placeholder={newDialog === "folder" ? "e.g. assets" : "e.g. app.js"}
                autoFocus
                className="mt-1 w-full rounded border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1 text-xs text-white"
              />
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <button
                onClick={() => setNewDialog(null)}
                className="rounded px-2.5 py-1 text-xs text-zinc-400 hover:text-white"
              >
                Cancel
              </button>
              <button
                onClick={handleCreateItem}
                disabled={!newItemName.trim()}
                className="rounded bg-zinc-100 px-3 py-1 text-xs font-bold text-zinc-950 disabled:opacity-40"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation */}
      {deleteConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="w-80 rounded-xl border border-red-900/50 bg-[var(--surface)] p-4 shadow-2xl space-y-3">
            <h4 className="text-sm font-semibold text-red-300">Delete Item</h4>
            <p className="text-xs text-zinc-300">
              Are you sure you want to delete <strong className="text-white">{deleteConfirm.name}</strong>?
            </p>
            <div className="flex justify-end gap-2 pt-2">
              <button
                onClick={() => setDeleteConfirm(null)}
                className="rounded px-2.5 py-1 text-xs text-zinc-400 hover:text-white"
              >
                Cancel
              </button>
              <button
                onClick={() => handleDeleteItem(deleteConfirm)}
                className="rounded bg-red-600 hover:bg-red-500 px-3 py-1 text-xs font-bold text-white"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
