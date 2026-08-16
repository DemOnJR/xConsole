import { useState, useMemo, useEffect } from "react";
import { NodeResizer, type NodeProps } from "@xyflow/react";
import { useCanvasStore, type PreviewNode } from "../stores/canvasStore";

type ViewportMode = "responsive" | "tablet" | "mobile";
type ViewTab = "preview" | "editor";

export function PreviewNode({ id, data, selected }: NodeProps<PreviewNode>) {
  const [viewTab, setViewTab] = useState<ViewTab>("preview");
  const [viewportMode, setViewportMode] = useState<ViewportMode>("responsive");
  const [key, setKey] = useState(0);
  const [copied, setCopied] = useState(false);
  const [liveCode, setLiveCode] = useState(data.html || "");

  const updateNodeData = useCanvasStore((s) => s.updateNodeData);
  const removeNode = useCanvasStore((s) => s.removeNode);
  const isTileMode = useCanvasStore((s) => s.layoutMode === "tile");

  const title = data.title || "Live HTML Sandbox";

  // Keep local editor in sync with external updates to data.html
  useEffect(() => {
    setLiveCode(data.html || "");
  }, [data.html]);

  // Build sandboxed HTML with Tailwind CSS and Inter font auto-injected if not present
  const fullHtml = useMemo(() => {
    const raw = data.html || "";
    if (raw.includes("<html") || raw.includes("<head")) {
      return raw;
    }
    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <script src="https://cdn.tailwindcss.com"></script>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
  <style>
    * { box-sizing: border-box; }
    body {
      font-family: 'Inter', system-ui, -apple-system, sans-serif;
      margin: 0;
      padding: 1rem;
      color: #f1f5f9;
      background-color: #0f172a;
      min-height: 100vh;
    }
  </style>
</head>
<body>
  ${raw}
</body>
</html>`;
  }, [data.html]);

  const reload = () => {
    setKey((k) => k + 1);
  };

  const copyHtml = async () => {
    await navigator.clipboard.writeText(data.html || "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const openInNewTab = () => {
    const blob = new Blob([fullHtml], { type: "text/html" });
    const url = URL.createObjectURL(blob);
    window.open(url, "_blank");
  };

  const handleCodeChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    setLiveCode(val);
    updateNodeData(id, { html: val });
  };

  const viewportWidths: Record<ViewportMode, string> = {
    responsive: "w-full",
    tablet: "w-[768px] max-w-full",
    mobile: "w-[375px] max-w-full",
  };

  return (
    <div
      className={`group/preview relative flex h-full w-full flex-col overflow-hidden rounded-xl border bg-[#090d16] text-gray-100 shadow-2xl transition-all ${
        selected
          ? "border-blue-500/80 ring-2 ring-blue-500/30"
          : "border-[var(--border)] hover:border-gray-600"
      }`}
    >
      {!isTileMode && (
        <NodeResizer
          minWidth={380}
          minHeight={260}
          isVisible={selected}
          lineClassName="!border-blue-500/50"
          handleClassName="!h-2.5 !w-2.5 !rounded-full !border-blue-500 !bg-blue-600"
        />
      )}

      {/* Title / Toolbar Bar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] bg-[#0d1322] px-3 py-2 select-none">
        <div className="flex items-center gap-2 min-w-0">
          <span className="flex items-center gap-1.5 rounded bg-blue-500/10 px-2 py-0.5 font-mono text-[11px] font-medium text-blue-300 border border-blue-500/20">
            <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
              <line x1="8" y1="21" x2="16" y2="21" />
              <line x1="12" y1="17" x2="12" y2="21" />
            </svg>
            Preview Sandbox
          </span>
          <span className="truncate text-[12px] font-semibold text-gray-200" title={title}>
            {title}
          </span>
        </div>

        {/* Viewport Presets & Controls */}
        <div className="flex items-center gap-1.5">
          {/* Tab Switcher (Preview vs Code Editor) */}
          <div className="flex items-center rounded-lg border border-[var(--border)] bg-[#1e293b]/60 p-0.5">
            <button
              type="button"
              onClick={() => setViewTab("preview")}
              className={`rounded px-2 py-0.5 text-[11px] transition ${
                viewTab === "preview"
                  ? "bg-blue-500/20 text-blue-300 font-medium"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              Preview
            </button>
            <button
              type="button"
              onClick={() => setViewTab("editor")}
              className={`rounded px-2 py-0.5 text-[11px] transition ${
                viewTab === "editor"
                  ? "bg-blue-500/20 text-blue-300 font-medium"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              Edit Code
            </button>
          </div>

          {/* Viewport Switcher */}
          {viewTab === "preview" && (
            <div className="flex items-center rounded-lg border border-[var(--border)] bg-[#1e293b]/60 p-0.5">
              <button
                type="button"
                onClick={() => setViewportMode("responsive")}
                className={`rounded px-1.5 py-0.5 text-[10px] transition ${
                  viewportMode === "responsive"
                    ? "bg-blue-500/20 text-blue-300 font-medium"
                    : "text-gray-400 hover:text-gray-200"
                }`}
                title="Responsive (100% width)"
              >
                Desktop
              </button>
              <button
                type="button"
                onClick={() => setViewportMode("tablet")}
                className={`rounded px-1.5 py-0.5 text-[10px] transition ${
                  viewportMode === "tablet"
                    ? "bg-blue-500/20 text-blue-300 font-medium"
                    : "text-gray-400 hover:text-gray-200"
                }`}
                title="Tablet (768px)"
              >
                Tablet
              </button>
              <button
                type="button"
                onClick={() => setViewportMode("mobile")}
                className={`rounded px-1.5 py-0.5 text-[10px] transition ${
                  viewportMode === "mobile"
                    ? "bg-blue-500/20 text-blue-300 font-medium"
                    : "text-gray-400 hover:text-gray-200"
                }`}
                title="Mobile (375px)"
              >
                Mobile
              </button>
            </div>
          )}

          {/* Reload Button */}
          <button
            type="button"
            onClick={reload}
            className="rounded p-1 text-gray-400 hover:bg-[#1e293b] hover:text-gray-200"
            title="Reload Sandbox Frame"
          >
            <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M23 4v6h-6M1 20v-6h6" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
          </button>

          {/* Copy HTML */}
          <button
            type="button"
            onClick={copyHtml}
            className="rounded px-1.5 py-0.5 text-[11px] text-gray-400 hover:bg-[#1e293b] hover:text-gray-200"
            title="Copy Raw HTML"
          >
            {copied ? "Copied ✓" : "HTML"}
          </button>

          {/* Pop-out Tab */}
          <button
            type="button"
            onClick={openInNewTab}
            className="rounded p-1 text-gray-400 hover:bg-[#1e293b] hover:text-gray-200"
            title="Open in new window"
          >
            <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
              <polyline points="15 3 21 3 21 9" />
              <line x1="10" y1="14" x2="21" y2="3" />
            </svg>
          </button>

          {/* Close Node */}
          <button
            type="button"
            onClick={() => removeNode(id)}
            className="rounded p-1 text-gray-400 hover:bg-red-500/20 hover:text-red-300"
            title="Close Preview Node"
          >
            <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      </div>

      {/* Main Viewport / Code Editor Area */}
      {viewTab === "editor" ? (
        <div className="relative flex flex-1 flex-col bg-[#070a12] p-2.5">
          <div className="mb-1.5 flex items-center justify-between text-[11px] text-gray-400 font-mono">
            <span>Live HTML / CSS / JS Source (auto-syncs with canvas)</span>
            <span className="text-[10px] text-emerald-400">● Live syncing</span>
          </div>
          <textarea
            value={liveCode}
            onChange={handleCodeChange}
            placeholder="Write HTML / CSS / JS code here..."
            spellCheck={false}
            className="flex-1 w-full resize-none rounded-lg border border-[var(--border)] bg-[#0c121e] p-3 font-mono text-[12px] leading-relaxed text-emerald-200/90 focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
          />
        </div>
      ) : (
        <div className="relative flex flex-1 items-center justify-center overflow-auto bg-[#070a12] p-2">
          <div className={`h-full transition-all shadow-inner overflow-hidden rounded-md border border-[var(--border)]/60 bg-white ${viewportWidths[viewportMode]}`}>
            <iframe
              key={key}
              srcDoc={fullHtml}
              title={title}
              sandbox="allow-scripts allow-forms allow-same-origin allow-modals allow-popups"
              className="h-full w-full border-0 bg-transparent"
            />
          </div>
        </div>
      )}
    </div>
  );
}

