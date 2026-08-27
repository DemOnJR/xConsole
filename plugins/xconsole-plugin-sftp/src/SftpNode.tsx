import { useState } from "react";
import { FolderIcon, FileIcon, RefreshIcon } from "./icons";
import type { SftpFileEntry } from "./types";

export function SftpNode() {
  const [currentPath] = useState("/var/www");
  const [files] = useState<SftpFileEntry[]>([
    { name: "..", path: "/var", is_dir: true, size: 0, modified: Date.now() },
    { name: "html", path: "/var/www/html", is_dir: true, size: 4096, modified: Date.now() },
    { name: "index.html", path: "/var/www/index.html", is_dir: false, size: 1024, modified: Date.now() },
    { name: "package.json", path: "/var/www/package.json", is_dir: false, size: 512, modified: Date.now() },
  ]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--surface)] shadow-lg font-mono">
      <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-3 py-2 text-xs">
        <div className="flex items-center gap-2">
          <FolderIcon size={14} className="text-amber-400" />
          <span className="font-semibold text-gray-200">SFTP Dual-Pane File Manager</span>
        </div>

        <div className="flex items-center gap-1 text-[11px] text-gray-400">
          <span className="rounded bg-black/40 px-2 py-0.5 border border-[var(--border)]">
            {currentPath}
          </span>
          <button className="p-1 hover:text-white rounded" title="Refresh">
            <RefreshIcon size={12} />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-auto p-2">
        <table className="w-full text-left text-xs">
          <thead>
            <tr className="text-gray-400 border-b border-[var(--border)]">
              <th className="p-1.5">Name</th>
              <th className="p-1.5">Size</th>
              <th className="p-1.5">Type</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--border)]/30">
            {files.map((file) => (
              <tr key={file.path} className="hover:bg-[var(--surface-hover)] cursor-pointer">
                <td className="p-1.5 flex items-center gap-2">
                  {file.is_dir ? (
                    <FolderIcon size={12} className="text-amber-400 shrink-0" />
                  ) : (
                    <FileIcon size={12} className="text-cyan-400 shrink-0" />
                  )}
                  <span className="truncate">{file.name}</span>
                </td>
                <td className="p-1.5 text-gray-400">{file.is_dir ? "-" : `${file.size} B`}</td>
                <td className="p-1.5 text-gray-500">{file.is_dir ? "Directory" : "File"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
