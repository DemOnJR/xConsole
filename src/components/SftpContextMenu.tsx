import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { ArchiveFormat, SftpEntry } from "../lib/tauri";

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
  /** Directory → one archive, built on the server then transferred. */
  onDownloadArchive: (entry: SftpEntry, format: ArchiveFormat) => void;
  onUpload: () => void;
  onProperties: (entry: SftpEntry) => void;
  onRename: (entry: SftpEntry) => void;
  onDelete: (entry: SftpEntry) => void;
  onCopyPath: (path: string) => void;
  onNewFolder: () => void;
  onNewFile: () => void;
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
  onRename,
  onDelete,
  onCopyPath,
  onNewFolder,
  onNewFile,
  onRefresh,
  externalEditorName,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);

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
              {item("Edit…", () => onEdit(entry))}
              {externalEditorName
                ? item(`Open in ${externalEditorName}`, () => onEditExternal(entry))
                : null}
              {item("Download", () => onDownload(entry))}
            </>
          )}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Upload here…", onUpload)}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Properties…", () => onProperties(entry))}
          {item("Rename…", () => onRename(entry))}
          {item("Copy path", () => onCopyPath(entry.path))}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Delete", () => onDelete(entry), { danger: true })}
        </>
      ) : (
        <>
          {item("Upload here…", onUpload)}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("New directory…", onNewFolder)}
          {item("New file…", onNewFile)}
          <div className="my-1 border-t border-[var(--border)]" />
          {item("Refresh", onRefresh)}
        </>
      )}
    </div>,
    document.body,
  );
}
