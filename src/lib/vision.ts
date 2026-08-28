import type { ChatImage } from "./tauri";

export type VisionMode = "ask" | "enabled" | "disabled";

export const VISION_MODE_KEY = "agent.vision_mode";
export const VISION_PROVIDER_KEY = "agent.vision_provider";
export const VISION_MODEL_KEY = "agent.vision_model";

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|bmp|svg|ico|avif|tiff)$/i;
const MAX_EDGE = 2048;
const MAX_BYTES = 8 * 1024 * 1024;

export function parseVisionMode(raw: string | undefined): VisionMode {
  const v = (raw ?? "").trim().toLowerCase();
  if (v === "enabled" || v === "on" || v === "true") return "enabled";
  if (v === "disabled" || v === "off" || v === "false") return "disabled";
  return "ask";
}

export function isImagePath(path: string): boolean {
  return IMAGE_EXT.test(path.trim());
}

export function mimeFromName(name: string): string {
  const ext = name.trim().toLowerCase().split(".").pop() ?? "";
  if (ext === "jpg" || ext === "jpeg") return "image/jpeg";
  if (ext === "png") return "image/png";
  if (ext === "gif") return "image/gif";
  if (ext === "webp") return "image/webp";
  if (ext === "svg") return "image/svg+xml";
  if (ext === "bmp") return "image/bmp";
  if (ext === "ico") return "image/x-icon";
  if (ext === "avif") return "image/avif";
  if (ext === "tiff" || ext === "tif") return "image/tiff";
  return "";
}

export function fileBaseName(path: string): string {
  const trimmed = path.trim().replace(/\\/g, "/");
  const parts = trimmed.split("/");
  return parts[parts.length - 1] || "image.png";
}

/** Keep `[Image #n]` markers in the visible text (Command Code). */
export function appendImageMarkers(text: string, count: number): string {
  if (count <= 0) return text;
  const existing = text.match(/\[Image #\d+\]/g)?.length ?? 0;
  if (existing >= count) return text;
  const extra: string[] = [];
  for (let i = existing + 1; i <= count; i++) extra.push(`[Image #${i}]`);
  const markers = extra.join(" ");
  const trimmed = text.trim();
  return trimmed ? `${trimmed}\n${markers}` : markers;
}

export function modelHasNativeVision(
  kind: string | undefined,
  model: string | undefined,
  baseUrl?: string | undefined,
): boolean {
  const k = (kind ?? "").toLowerCase();
  const m = (model ?? "").toLowerCase();
  const url = (baseUrl ?? "").toLowerCase();
  if (k === "anthropic") return true;
  if (url.includes("generativelanguage.googleapis.com") || m.includes("gemini")) return true;
  if (k.includes("cli") || k === "cursor") return false;
  const hints = [
    "gpt-4o",
    "gpt-4.1",
    "gpt-4-turbo",
    "gpt-4-vision",
    "gpt-5",
    "o1",
    "o3",
    "o4",
    "claude",
    "grok-2",
    "grok-3",
    "grok-4",
    "qwen-vl",
    "qwen2-vl",
    "qwen2.5-vl",
    "qwen3-vl",
    "pixtral",
    "llama-4",
    "llama4",
    "glm-4v",
  ];
  return hints.some((h) => m.includes(h));
}

export function isGeminiProvider(p: {
  name?: string | null;
  base_url?: string | null;
}): boolean {
  const name = (p.name ?? "").toLowerCase();
  const url = (p.base_url ?? "").toLowerCase();
  return name.includes("gemini") || url.includes("generativelanguage.googleapis.com");
}

export function defaultVisionModel(
  p: { name?: string | null; base_url?: string | null; model?: string | null },
  override?: string,
): string {
  const over = (override ?? "").trim();
  if (over) return over;
  if (isGeminiProvider(p)) return "gemini-2.5-flash";
  return (p.model ?? "").trim() || "gpt-4o-mini";
}

export function visionLabel(
  mode: VisionMode,
  providerName?: string,
  model?: string,
): string {
  if (mode === "disabled") return "vision off";
  const who = [providerName, model].filter(Boolean).join(" · ");
  if (mode === "ask") return who ? `vision ask · ${who}` : "vision ask";
  return who ? `vision · ${who}` : "vision on";
}

export async function bytesToChatImage(
  bytes: Uint8Array,
  name: string,
  mimeHint?: string,
): Promise<ChatImage> {
  const mime = (mimeHint && mimeHint.startsWith("image/") ? mimeHint : mimeFromName(name)) || "image/png";
  const blob = new Blob([bytes as BlobPart], { type: mime });
  return normalizeImageBlob(blob, name, mime);
}

export async function fileToChatImage(file: File): Promise<ChatImage> {
  return normalizeImageBlob(file, file.name || "image.png", file.type || mimeFromName(file.name) || "image/png");
}

async function normalizeImageBlob(blob: Blob, name: string, mime: string): Promise<ChatImage> {
  const bitmap = await blobToBitmap(blob);
  if (!bitmap) {
    const data = await blobToBase64(blob);
    return { media_type: mime || "image/png", data, name };
  }
  const scale = Math.min(1, MAX_EDGE / Math.max(bitmap.width, bitmap.height));
  const w = Math.max(1, Math.round(bitmap.width * scale));
  const h = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    const data = await blobToBase64(blob);
    return { media_type: mime || "image/png", data, name };
  }
  ctx.drawImage(bitmap, 0, 0, w, h);
  bitmap.close?.();
  const outMime = mime === "image/jpeg" || mime === "image/jpg" ? "image/jpeg" : "image/png";
  let quality = 0.9;
  let dataUrl = canvas.toDataURL(outMime, quality);
  while (dataUrl.length > MAX_BYTES * 1.37 && quality > 0.5) {
    quality -= 0.1;
    dataUrl = canvas.toDataURL("image/jpeg", quality);
  }
  const comma = dataUrl.indexOf(",");
  const data = comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
  const finalMime = dataUrl.startsWith("data:image/jpeg") ? "image/jpeg" : outMime;
  return { media_type: finalMime, data, name };
}

async function blobToBitmap(blob: Blob): Promise<ImageBitmap | null> {
  try {
    return await createImageBitmap(blob);
  } catch {
    return null;
  }
}

async function blobToBase64(blob: Blob): Promise<string> {
  const buf = await blob.arrayBuffer();
  const bytes = new Uint8Array(buf);
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
}

export function previewSrc(img: ChatImage): string {
  return `data:${img.media_type || "image/png"};base64,${img.data}`;
}

/** All files sitting on a paste/drop DataTransfer, partitioned into images and other files. */
export function filesFromClipboardEvent(data: DataTransfer | null | undefined): { images: File[]; files: File[] } {
  if (!data) return { images: [], files: [] };
  const images: File[] = [];
  const files: File[] = [];
  const seen = new Set<string>();
  const add = (file: File | null | undefined) => {
    if (!file) return;
    const type = (file.type || mimeFromName(file.name)).toLowerCase();
    const key = `${file.name}:${file.size}:${type}`;
    if (seen.has(key)) return;
    seen.add(key);
    if (type.startsWith("image/") || isImagePath(file.name)) {
      images.push(file);
    } else {
      files.push(file);
    }
  };
  for (const file of Array.from(data.files ?? [])) add(file);
  for (const item of Array.from(data.items ?? [])) {
    if (item.kind === "file") add(item.getAsFile());
  }
  return { images, files };
}

/** Image files sitting on a paste/drop DataTransfer (browser clipboard). */
export function imagesFromClipboardEvent(data: DataTransfer | null | undefined): File[] {
  return filesFromClipboardEvent(data).images;
}

/** Parse candidate absolute local file paths from text/uri-list or text/plain. */
export function extractFilePathsFromClipboard(data: DataTransfer | null | undefined): string[] {
  if (!data) return [];
  const paths: string[] = [];
  const uriList = data.getData("text/uri-list") || "";
  if (uriList) {
    for (const line of uriList.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      if (trimmed.startsWith("file:///")) {
        let raw = trimmed.slice("file:///".length);
        try {
          raw = decodeURIComponent(raw);
        } catch {
          /* ignore decode error */
        }
        if (/^[a-zA-Z]:\//i.test(raw)) {
          paths.push(raw.replace(/\//g, "\\"));
        } else {
          paths.push("/" + raw);
        }
      } else if (trimmed.startsWith("file://")) {
        let raw = trimmed.slice("file://".length);
        try {
          raw = decodeURIComponent(raw);
        } catch {
          /* ignore */
        }
        paths.push(raw);
      }
    }
  }

  const text = data.getData("text/plain") || "";
  if (text && text.length < 4000) {
    const rawLines = text
      .split(/\r?\n/)
      .map((l) => l.trim().replace(/^["']|["']$/g, ""))
      .filter(Boolean);
    const isWindowsPath = (s: string) => /^[a-zA-Z]:[\\/][^<>:"|?*\n\r]+$/.test(s);
    const isUnixPath = (s: string) => /^\/[^<>:"|?*\n\r]+$/.test(s);
    if (
      rawLines.length > 0 &&
      rawLines.length <= 20 &&
      rawLines.every((l) => isWindowsPath(l) || isUnixPath(l) || l.startsWith("file://"))
    ) {
      for (const line of rawLines) {
        if (line.startsWith("file:///")) {
          let raw = line.slice("file:///".length);
          try {
            raw = decodeURIComponent(raw);
          } catch {
            /* ignore */
          }
          if (/^[a-zA-Z]:\//i.test(raw)) paths.push(raw.replace(/\//g, "\\"));
          else paths.push("/" + raw);
        } else if (!paths.includes(line)) {
          paths.push(line);
        }
      }
    }
  }

  return Array.from(new Set(paths));
}

/** True when the OS clipboard is likely a screenshot or copied file. */
export function clipboardLooksLikeImage(data: DataTransfer | null | undefined): boolean {
  if (!data) return false;
  if (imagesFromClipboardEvent(data).length > 0) return true;
  const types = Array.from(data.types ?? []).map((t) => t.toLowerCase());
  if (types.some((t) => t.startsWith("image/") || t === "files" || t.includes("png") || t.includes("dib"))) {
    return true;
  }
  const paths = extractFilePathsFromClipboard(data);
  if (paths.some((p) => isImagePath(p))) return true;
  const text = data.getData("text/plain")?.trim() ?? "";
  if (text && isImagePath(text)) return true;
  return false;
}

