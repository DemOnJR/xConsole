import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { ArchiveFormat, SftpEntry } from "../../../src/lib/tauri";

export interface SftpMenuState {
  x: number;
  y: number;
  entry: SftpEntry | null;
}

interface Props {
  menu: SftpMenuState;
  onClose: () => void;
  onOpen: (entry: SftpEntry) => void;
  onEdit: (entry: SftpEntry) => void;
  onEditExternal: (entry: SftpEntry) => void;
  onDownload: (entry: SftpEntry) => void;
  /** Directory GåÆ one archive, built on the server then transferred. */
  onDownloadArchive: (entry: SftpEntry, format: ArchiveFormat) => void;
  onUpload: () => void;
  onProperties: (entry: SftpEntry) => void;
  /** Optional: apply octal mode to the current selection (bulk chmod). */
  onChmodSelection?: (entry: SftpEntry) => void;
  onRename: (entry: SftpEntry) => void;
  onDuplicate?: (entry: SftpEntry) => void;
  onDelete: (entry: SftpEntry) => void;
  onCopyPath: (path: string) => void;
  /** Open (or focus) a terminal and cd into this directory. */
  onOpenTerminalHere: (entry: SftpEntry | null) => void;
  onNewFolder: () => void;
  onNewFile: () => void;
  /** How many rows are selected GÇö decides whether the menu talks about one or many. */
  selectionCount: number;
  onDownloadSelection: (entry: SftpEntry | null) => void;
  onDeleteSelection: (entry: SftpEntry | null) => void;
  onCopy: (entry: SftpEntry | null) => void;
  onCut: (entry: SftpEntry | null) => void;
  onPaste: () => void;
  /** Something is on the clipboard, so Paste is worth offering. */
  canPaste: boolean;
  /** Repoint an existing symlink. */
  onEditLink: (entry: SftpEntry) => void;
  /** Create a new symlink in the current directory. */
  onNewLink: () => void;
  onRefresh: () => void;
  /** Name of the configured external editor, e.g. "VS Code"; null hides the item. */
  externalEditorName: string | null;
}

export function SftpContextMenu({
  menu,
  onClose,
  onOpen,
  onEdit,
  onEditExternal,
  onDownload,
  onDownloadArchive,
  onUpload,
  onProperties,
  onChmodSelection,
  onRename,
  onDuplicate,
  onDelete,
  onCopyPath,
  onOpenTerminalHere,
  onNewFolder,
  onNewFile,
  selectionCount,
  onDownloadSelection,
  onDeleteSelection,
  onCopy,
  onCut,
  onPaste,
  canPaste,
  onEditLink,
  onNewLink,
  onRefresh,
  externalEditorName,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  /** The clicked row is part of a multi-row selection, so actions apply to all. */
  const many = selectionCount > 1;


  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const t = window.setTimeout(() => {
      document.addEventListener("mousedown", onDoc);
      document.addEventListener("keydown", onKey);
    }, 0);
    return () => {
      clearTimeout(t);
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const item = (label: string, action: () => void, opts?: { danger?: boolean; disabled?: boolean }) => (
    <button
      type="button"
      disabled={opts?.disabled}
      className={`block w-full px-3 py-1.5 text-left text-xs disabled:opacity-40 ${
        opts?.danger
          ? "text-red-300 hover:bg-red-950/40"
          : "text-gray-200 hover:bg-[var(--border)]"
      }`}
      onClick={() => {
        action();
        onClose();
      }}
    >
      {label}
    </button>
  );

  const entry = menu.entry;
  const maxX = window.innerWidth - 200;
  const maxY = window.innerHeight - 280;

  return createPortal(
    <div
      ref={ref}
      className="fixed z-[9998] min-w-[180px] overflow-hidden rounded-md border border-[var(--border)] bg-[var(--surface)] py-1 shadow-xl"
      style={{
        left: Math.min(menu.x, maxX),
        top: Math.min(menu.y, maxY),
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {entry ? (
        <>
          {entry.is_dir ? (
            <>
              {item("Open", () => onOpen(entry))}
              {/* A folder downloads file-by-file (progress per file, resumable by
                  retry) or as one archive (fewer round-trips, far faster on a tree
                  of many small files). Both are offered because neither wins always. */}
              {item("Download folder", () => onDownload(entry))}
              {item("Download as .tar.gz", () => onDownloadArchive(entry, "targz"))}
              {item("Download as .zip", () => onDownloadArchive(entry, "zip"))}
            </>
          ) : (
            <>
              {item("EditGÇª", () => onEdit(entry))}
              {externalEditorName
                ? item(`Open in ${externalEditorName}`, () => onEditExternal(entry))
                : null}
              {item("Download", () => onDownload(entry))}
            </>
          )}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Upload hereGÇª", onUpload)}
          <div className="my-1 border-t border-[var(--border)]" />
          {/* With several rows highlighted the menu acts on all of them, and says so GÇö
              a menu that silently applies to one of six is how the wrong file gets
              deleted. `many` is only true when the clicked row is part of that set;
              clicking outside it selects that row first. */}
          {many && (
            <>
              {item(`Download ${selectionCount} items`, () => onDownloadSelection(entry))}
              {item(`Copy ${selectionCount} items`, () => onCopy(entry))}
              {item(`Cut ${selectionCount} items`, () => onCut(entry))}
              {onChmodSelection
                ? item(`Chmod ${selectionCount} itemsGÇª`, () => onChmodSelection(entry))
                : null}
              {item(`Delete ${selectionCount} items`, () => onDeleteSelection(entry), {
                danger: true,
              })}
              <div className="my-1 border-t border-[var(--border)]" />
            </>
          )}
          {!many && (
            <>
              {item("Copy", () => onCopy(entry))}
              {item("Cut", () => onCut(entry))}
            </>
          )}
          {canPaste ? item("Paste here", onPaste) : null}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("PropertiesGÇª", () => onProperties(entry))}
          {onChmodSelection && !many
            ? item("ChmodGÇª", () => onChmodSelection(entry))
            : null}
          {/* Only for links: for anything else there is no target to point anywhere. */}
          {entry.is_symlink ? item("Edit link targetGÇª", () => onEditLink(entry)) : null}
          {item("RenameGÇª", () => onRename(entry))}
          {onDuplicate ? item("Duplicate", () => onDuplicate(entry)) : null}
          {item(
            many ? `Copy ${selectionCount} paths` : "Copy path",
            () => onCopyPath(entry.path),
          )}
          {item("Open terminal here", () => onOpenTerminalHere(entry))}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Delete", () => onDelete(entry), { danger: true })}
        </>
      ) : (
        <>
          {item("Upload hereGÇª", onUpload)}
          {canPaste ? item("Paste here", onPaste) : null}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Open terminal here", () => onOpenTerminalHere(null))}
          {item("New directoryGÇª", onNewFolder)}
          {item("New fileGÇª", onNewFile)}
          {item("New symlinkGÇª", onNewLink)}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Refresh", onRefresh)}
        </>
      )}
    </div>,
    document.body,
  );
}
