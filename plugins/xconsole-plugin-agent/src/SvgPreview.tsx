import React, { useState, useRef, useMemo, useCallback } from "react";
import { CodeHighlight } from "./SyntaxHighlight";

interface SvgPreviewProps {
  svgContent: string;
  name?: string;
  className?: string;
  initialHeight?: number;
}

type ThemeMode = "dark-grid" | "light-grid" | "checkered" | "solid-dark" | "solid-light";

export function SvgPreview({
  svgContent,
  name = "graphic",
  className = "",
  initialHeight = 340,
}: SvgPreviewProps) {
  const [showCode, setShowCode] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = useState(false);
  const [startPan, setStartPan] = useState({ x: 0, y: 0 });
  const [theme, setTheme] = useState<ThemeMode>("dark-grid");
  const [copied, setCopied] = useState(false);
  const [exporting, setExporting] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const svgWrapperRef = useRef<HTMLDivElement>(null);

  // Validate and sanitize SVG XML
  const { cleanSvg, isValid, errorMsg, dimensions } = useMemo(() => {
    try {
      const parser = new DOMParser();
      const doc = parser.parseFromString(svgContent, "image/svg+xml");
      const parserError = doc.querySelector("parsererror");
      if (parserError) {
        return { cleanSvg: svgContent, isValid: false, errorMsg: parserError.textContent || "Invalid XML", dimensions: null };
      }
      const svgEl = doc.querySelector("svg");
      if (!svgEl) {
        return { cleanSvg: svgContent, isValid: false, errorMsg: "Missing root <svg> element", dimensions: null };
      }

      // Ensure responsiveness
      if (!svgEl.getAttribute("xmlns")) {
        svgEl.setAttribute("xmlns", "http://www.w3.org/2000/svg");
      }
      const viewBox = svgEl.getAttribute("viewBox");
      const width = svgEl.getAttribute("width");
      const height = svgEl.getAttribute("height");

      return {
        cleanSvg: doc.documentElement.outerHTML,
        isValid: true,
        errorMsg: null,
        dimensions: { viewBox, width, height },
      };
    } catch (e: any) {
      return { cleanSvg: svgContent, isValid: false, errorMsg: e.message || "Failed to parse SVG", dimensions: null };
    }
  }, [svgContent]);

  // Handle zoom with limits
  const handleZoom = (delta: number) => {
    setZoom((prev) => Math.min(Math.max(0.25, +(prev + delta).toFixed(2)), 5));
  };

  const resetView = () => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  };

  // Pan interaction
  const handleMouseDown = (e: React.MouseEvent) => {
    if (showCode) return;
    setIsPanning(true);
    setStartPan({ x: e.clientX - pan.x, y: e.clientY - pan.y });
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!isPanning || showCode) return;
    setPan({ x: e.clientX - startPan.x, y: e.clientY - startPan.y });
  };

  const handleMouseUp = () => {
    setIsPanning(false);
  };

  const handleWheel = (e: React.WheelEvent) => {
    if (showCode) return;
    e.preventDefault();
    const delta = e.deltaY < 0 ? 0.15 : -0.15;
    handleZoom(delta);
  };

  // Copy SVG Code
  const copySvg = async () => {
    await navigator.clipboard.writeText(svgContent);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  // Download .svg file
  const downloadSvg = () => {
    const blob = new Blob([svgContent], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${name.replace(/\.svg$/, "")}.svg`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  // Export as PNG via HTML5 Canvas
  const exportPng = useCallback(() => {
    if (!isValid || exporting) return;
    setExporting(true);

    try {
      const blob = new Blob([cleanSvg], { type: "image/svg+xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const img = new Image();

      img.onload = () => {
        const scale = 2; // 2x resolution for crisp high-DPI output
        const canvas = document.createElement("canvas");
        const w = (img.naturalWidth || 800) * scale;
        const h = (img.naturalHeight || 600) * scale;
        canvas.width = w;
        canvas.height = h;

        const ctx = canvas.getContext("2d");
        if (ctx) {
          ctx.drawImage(img, 0, 0, w, h);
          const pngUrl = canvas.toDataURL("image/png");
          const a = document.createElement("a");
          a.href = pngUrl;
          a.download = `${name.replace(/\.svg$/, "")}@2x.png`;
          document.body.appendChild(a);
          a.click();
          document.body.removeChild(a);
        }
        URL.revokeObjectURL(url);
        setExporting(false);
      };

      img.onerror = () => {
        URL.revokeObjectURL(url);
        setExporting(false);
      };

      img.src = url;
    } catch {
      setExporting(false);
    }
  }, [cleanSvg, isValid, exporting, name]);

  const bgClasses: Record<ThemeMode, string> = {
    "dark-grid": "bg-[#0b101b] bg-[radial-gradient(#1e293b_1px,transparent_1px)] [background-size:16px_16px]",
    "light-grid": "bg-[#f8fafc] bg-[radial-gradient(#cbd5e1_1px,transparent_1px)] [background-size:16px_16px]",
    "checkered": "bg-[#111827] [background-image:linear-gradient(45deg,#1f2937_25%,transparent_25%),linear-gradient(-45deg,#1f2937_25%,transparent_25%),linear-gradient(45deg,transparent_75%,#1f2937_75%),linear-gradient(-45deg,transparent_75%,#1f2937_75%)] [background-size:20px_20px] [background-position:0_0,0_10px,10px_-10px,-10px_0px]",
    "solid-dark": "bg-[#0a0e17]",
    "solid-light": "bg-white",
  };

  return (
    <div
      className={`group relative my-3 overflow-hidden rounded-xl border border-[var(--border)] bg-[#070a10] shadow-xl transition-all ${className}`}
    >
      {/* Top Controls Bar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] bg-[#0f172a]/80 px-3.5 py-2 backdrop-blur-sm">
        <div className="flex items-center gap-2">
          <span className="flex items-center gap-1.5 rounded bg-emerald-500/10 px-2 py-0.5 font-mono text-[11px] font-medium text-emerald-300 border border-emerald-500/20">
            <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
            </svg>
            SVG Vector Graphic
          </span>
          {dimensions?.viewBox && (
            <span className="hidden sm:inline font-mono text-[10px] text-gray-500">
              viewBox: {dimensions.viewBox}
            </span>
          )}
        </div>

        {/* Action Buttons */}
        <div className="flex items-center gap-1.5">
          {/* Theme background switcher */}
          {!showCode && (
            <div className="flex items-center rounded-lg border border-[var(--border)] bg-[#1e293b]/60 p-0.5">
              <button
                type="button"
                onClick={() => setTheme("dark-grid")}
                title="Dark Grid"
                className={`rounded px-1.5 py-0.5 text-[10px] transition ${theme === "dark-grid" ? "bg-blue-500/20 text-blue-300 font-medium" : "text-gray-400 hover:text-gray-200"}`}
              >
                Dark
              </button>
              <button
                type="button"
                onClick={() => setTheme("light-grid")}
                title="Light Grid"
                className={`rounded px-1.5 py-0.5 text-[10px] transition ${theme === "light-grid" ? "bg-blue-500/20 text-blue-300 font-medium" : "text-gray-400 hover:text-gray-200"}`}
              >
                Light
              </button>
              <button
                type="button"
                onClick={() => setTheme("checkered")}
                title="Checkered Alpha"
                className={`rounded px-1.5 py-0.5 text-[10px] transition ${theme === "checkered" ? "bg-blue-500/20 text-blue-300 font-medium" : "text-gray-400 hover:text-gray-200"}`}
              >
                Alpha
              </button>
            </div>
          )}

          {/* Zoom controls */}
          {!showCode && (
            <div className="flex items-center rounded-lg border border-[var(--border)] bg-[#1e293b]/60 p-0.5">
              <button
                type="button"
                onClick={() => handleZoom(-0.25)}
                className="px-1.5 py-0.5 text-[11px] text-gray-400 hover:text-gray-100"
                title="Zoom Out"
              >
                −
              </button>
              <span className="min-w-[36px] text-center font-mono text-[10px] text-gray-300">
                {Math.round(zoom * 100)}%
              </span>
              <button
                type="button"
                onClick={() => handleZoom(0.25)}
                className="px-1.5 py-0.5 text-[11px] text-gray-400 hover:text-gray-100"
                title="Zoom In"
              >
                +
              </button>
              <button
                type="button"
                onClick={resetView}
                className="ml-0.5 border-l border-gray-700/60 pl-1.5 pr-1 py-0.5 text-[10px] text-gray-400 hover:text-gray-200"
                title="Reset View"
              >
                Fit
              </button>
            </div>
          )}

          {/* Toggle Code/Visual */}
          <button
            type="button"
            onClick={() => setShowCode(!showCode)}
            className={`rounded-lg border px-2 py-1 text-[11px] transition ${showCode ? "border-blue-500/50 bg-blue-500/10 text-blue-300" : "border-[var(--border)] bg-[#1e293b]/60 text-gray-300 hover:bg-[#334155]"}`}
          >
            {showCode ? "Visual Preview" : "View Code"}
          </button>

          {/* Export PNG */}
          <button
            type="button"
            onClick={exportPng}
            disabled={exporting}
            className="rounded-lg border border-[var(--border)] bg-[#1e293b]/60 px-2 py-1 text-[11px] text-gray-300 hover:bg-[#334155] hover:text-gray-100 disabled:opacity-50"
            title="Export High-Res PNG (@2x)"
          >
            {exporting ? "Exporting…" : "PNG @2x"}
          </button>

          {/* Download SVG */}
          <button
            type="button"
            onClick={downloadSvg}
            className="rounded-lg border border-[var(--border)] bg-[#1e293b]/60 px-2 py-1 text-[11px] text-gray-300 hover:bg-[#334155] hover:text-gray-100"
            title="Download .svg"
          >
            .SVG
          </button>

          {/* Copy Button */}
          <button
            type="button"
            onClick={() => void copySvg()}
            className="rounded-lg border border-[var(--border)] bg-[#1e293b]/60 px-2 py-1 text-[11px] text-gray-300 hover:bg-[#334155] hover:text-gray-100"
          >
            {copied ? "Copied ✓" : "Copy"}
          </button>
        </div>
      </div>

      {/* Main Preview Area */}
      {showCode ? (
        <div className="max-h-[500px] overflow-auto p-3 font-mono text-[12px]">
          <CodeHighlight code={svgContent} language="xml" />
        </div>
      ) : isValid ? (
        <div
          ref={containerRef}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
          onWheel={handleWheel}
          className={`relative flex items-center justify-center overflow-hidden cursor-grab active:cursor-grabbing select-none ${bgClasses[theme]}`}
          style={{ height: `${initialHeight}px` }}
        >
          <div
            ref={svgWrapperRef}
            style={{
              transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
              transformOrigin: "center center",
              transition: isPanning ? "none" : "transform 0.1s ease-out",
            }}
            className="flex items-center justify-center p-6"
            dangerouslySetInnerHTML={{ __html: cleanSvg }}
          />

          {/* Bottom Hint */}
          <div className="pointer-events-none absolute bottom-2 right-3 font-mono text-[10px] text-gray-500/70 bg-black/40 px-2 py-0.5 rounded backdrop-blur-sm">
            Scroll to zoom · Drag to pan
          </div>
        </div>
      ) : (
        <div className="p-4 text-center">
          <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-[12px] text-red-300">
            <strong>SVG Parse Error:</strong> {errorMsg}
          </div>
          <pre className="mt-3 max-h-[300px] overflow-auto rounded bg-black/50 p-2 text-left font-mono text-[11px] text-gray-400">
            {svgContent}
          </pre>
        </div>
      )}
    </div>
  );
}

