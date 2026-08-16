import { useState } from "react";
import { useAgentStore } from "../../stores/agentStore";
import { invoke } from "@tauri-apps/api/core";

interface ImagePreviewProps {
  src: string;
  alt?: string;
  width?: number;
  height?: number;
  provider?: string;
  className?: string;
}

export function ImagePreview({
  src,
  alt = "Generated Image",
  width,
  height,
  provider,
  className = "",
}: ImagePreviewProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadMsg, setUploadMsg] = useState<string | null>(null);

  const targets = useAgentStore((s) => s.targets);

  const cleanPath = src.replace(/^file:\/\/\//, "").replace(/^file:\/\//, "");

  const copyPath = async () => {
    await navigator.clipboard.writeText(cleanPath);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const uploadToVps = async () => {
    if (targets.length === 0) {
      setUploadMsg("No VPS selected");
      setTimeout(() => setUploadMsg(null), 2500);
      return;
    }
    const vpsId = targets[0];
    setUploading(true);
    try {
      const filename = cleanPath.split(/[/\\]/).pop() || "image.png";
      const remotePath = `/tmp/${filename}`;
      await invoke("sftp_upload_file", {
        vpsId,
        localPath: cleanPath,
        remotePath,
      });
      setUploadMsg(`Uploaded to ${remotePath}`);
      setTimeout(() => setUploadMsg(null), 3000);
    } catch (e: any) {
      setUploadMsg(`Error: ${e?.message || e}`);
      setTimeout(() => setUploadMsg(null), 3500);
    } finally {
      setUploading(false);
    }
  };

  return (
    <div className={`my-3 overflow-hidden rounded-xl border border-[var(--border)] bg-[#070a10] shadow-lg ${className}`}>
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] bg-[#0f172a]/80 px-3.5 py-1.5 backdrop-blur-sm">
        <div className="flex items-center gap-2">
          <span className="flex items-center gap-1.5 rounded bg-purple-500/10 px-2 py-0.5 font-mono text-[11px] font-medium text-purple-300 border border-purple-500/20">
            <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
              <circle cx="8.5" cy="8.5" r="1.5" />
              <polyline points="21 15 16 10 5 21" />
            </svg>
            Generated Image
          </span>
          {provider && (
            <span className="font-mono text-[10px] text-gray-400">
              via {provider}
            </span>
          )}
          {width && height && (
            <span className="font-mono text-[10px] text-gray-500">
              {width}×{height}
            </span>
          )}
        </div>

        {/* Quick Actions */}
        <div className="flex items-center gap-1.5">
          {uploadMsg ? (
            <span className="font-mono text-[10px] text-emerald-400 bg-emerald-950/60 px-2 py-0.5 rounded border border-emerald-500/30">
              {uploadMsg}
            </span>
          ) : (
            targets.length > 0 && (
              <button
                type="button"
                onClick={uploadToVps}
                disabled={uploading}
                className="flex items-center gap-1 rounded-lg border border-[var(--border)] bg-[#1e293b]/60 px-2 py-1 text-[11px] text-gray-300 hover:bg-[#334155] hover:text-gray-100 disabled:opacity-50"
                title={`Upload to selected VPS (${targets[0].slice(0, 8)}…)`}
              >
                <svg className="w-3 h-3 text-blue-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                  <polyline points="17 8 12 3 7 8" />
                  <line x1="12" y1="3" x2="12" y2="15" />
                </svg>
                {uploading ? "Uploading…" : "Send to VPS"}
              </button>
            )
          )}

          <button
            type="button"
            onClick={copyPath}
            className="rounded-lg border border-[var(--border)] bg-[#1e293b]/60 px-2 py-1 text-[11px] text-gray-300 hover:bg-[#334155] hover:text-gray-100"
          >
            {copied ? "Path Copied ✓" : "Copy Path"}
          </button>
        </div>
      </div>

      {/* Image Thumbnail */}
      <div
        onClick={() => setIsOpen(true)}
        className="group/img relative flex cursor-zoom-in items-center justify-center bg-[#090d16] p-2 hover:bg-[#0c121e] transition"
      >
        <img
          src={src}
          alt={alt}
          className="max-h-[380px] w-auto max-w-full rounded-lg object-contain shadow transition group-hover/img:scale-[1.01]"
          loading="lazy"
        />
        <div className="pointer-events-none absolute bottom-3 right-4 rounded-md bg-black/70 px-2 py-1 font-mono text-[10px] text-gray-300 opacity-0 backdrop-blur-sm transition group-hover/img:opacity-100">
          Click to expand lightbox
        </div>
      </div>

      {alt && alt !== "Generated Image" && (
        <div className="border-t border-[var(--border)]/60 bg-[#0a0e17] px-3.5 py-1.5 font-mono text-[11px] text-gray-400 italic">
          "{alt}"
        </div>
      )}

      {/* Fullscreen Lightbox Modal */}
      {isOpen && (
        <div
          onClick={() => setIsOpen(false)}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/85 p-6 backdrop-blur-md cursor-zoom-out"
        >
          <div className="relative max-h-[90vh] max-w-[90vw] overflow-hidden rounded-2xl border border-gray-700 bg-[#0c121e] shadow-2xl">
            <button
              type="button"
              onClick={() => setIsOpen(false)}
              className="absolute right-3 top-3 z-10 rounded-full bg-black/70 p-1.5 text-gray-300 hover:bg-black hover:text-white"
            >
              <svg className="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
            <img
              src={src}
              alt={alt}
              className="max-h-[85vh] w-auto max-w-[85vw] object-contain"
            />
            {alt && (
              <div className="border-t border-gray-800 bg-[#080b12] p-3 text-center text-[12px] text-gray-300">
                {alt}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
