import type { ChatImage } from "./tauri";
export type VisionMode = "ask" | "enabled" | "disabled";
export declare const VISION_MODE_KEY = "agent.vision_mode";
export declare const VISION_PROVIDER_KEY = "agent.vision_provider";
export declare const VISION_MODEL_KEY = "agent.vision_model";
export declare function parseVisionMode(raw: string | undefined): VisionMode;
export declare function isImagePath(path: string): boolean;
export declare function mimeFromName(name: string): string;
export declare function fileBaseName(path: string): string;
/** Keep `[Image #n]` markers in the visible text (Command Code). */
export declare function appendImageMarkers(text: string, count: number): string;
export declare function modelHasNativeVision(kind: string | undefined, model: string | undefined, baseUrl?: string | undefined): boolean;
export declare function isGeminiProvider(p: {
    name?: string | null;
    base_url?: string | null;
}): boolean;
export declare function defaultVisionModel(p: {
    name?: string | null;
    base_url?: string | null;
    model?: string | null;
}, override?: string): string;
export declare function visionLabel(mode: VisionMode, providerName?: string, model?: string): string;
export declare function bytesToChatImage(bytes: Uint8Array, name: string, mimeHint?: string): Promise<ChatImage>;
export declare function fileToChatImage(file: File): Promise<ChatImage>;
export declare function previewSrc(img: ChatImage): string;
/** Image files sitting on a paste/drop DataTransfer (browser clipboard). */
export declare function imagesFromClipboardEvent(data: DataTransfer | null | undefined): File[];
/** True when the OS clipboard is likely a screenshot (WebView2 often omits the bytes). */
export declare function clipboardLooksLikeImage(data: DataTransfer | null | undefined): boolean;
//# sourceMappingURL=vision.d.ts.map