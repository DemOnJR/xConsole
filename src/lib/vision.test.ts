import { describe, expect, it } from "vitest";
import {
  appendImageMarkers,
  defaultVisionModel,
  fileBaseName,
  isGeminiProvider,
  isImagePath,
  mimeFromName,
  modelHasNativeVision,
  parseVisionMode,
  visionLabel,
  imagesFromClipboardEvent,
  clipboardLooksLikeImage,
} from "./vision";

describe("vision helpers", () => {
  it("defaults mode to ask", () => {
    expect(parseVisionMode(undefined)).toBe("ask");
    expect(parseVisionMode("")).toBe("ask");
    expect(parseVisionMode("enabled")).toBe("enabled");
    expect(parseVisionMode("off")).toBe("disabled");
  });

  it("detects image paths and mime types", () => {
    expect(isImagePath("C:\\\\Users\\\\a\\\\shot.PNG")).toBe(true);
    expect(isImagePath("/tmp/notes.txt")).toBe(false);
    expect(mimeFromName("x.jpg")).toBe("image/jpeg");
    expect(mimeFromName("x.webp")).toBe("image/webp");
    expect(fileBaseName("C:\\\\a\\\\b\\\\clip.png")).toBe("clip.png");
  });

  it("appends [Image #n] without duplicating", () => {
    expect(appendImageMarkers("look", 2)).toBe("look\n[Image #1] [Image #2]");
    expect(appendImageMarkers("[Image #1] already", 1)).toBe("[Image #1] already");
    expect(appendImageMarkers("", 1)).toBe("[Image #1]");
  });

  it("treats Claude / Gemini / GPT-4o as native vision", () => {
    expect(modelHasNativeVision("anthropic", "claude-sonnet-4-5")).toBe(true);
    expect(
      modelHasNativeVision(
        "openai",
        "gemini-2.5-flash",
        "https://generativelanguage.googleapis.com/v1beta/openai",
      ),
    ).toBe(true);
    expect(modelHasNativeVision("openai", "gpt-4o")).toBe(true);
    expect(modelHasNativeVision("openai", "deepseek-chat", "https://api.deepseek.com/v1")).toBe(
      false,
    );
  });

  it("prefers Gemini flash as the default vision model", () => {
    const gemini = {
      name: "Google Gemini",
      base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
      model: "gemini-2.5-pro",
    };
    expect(isGeminiProvider(gemini)).toBe(true);
    expect(defaultVisionModel(gemini)).toBe("gemini-2.5-flash");
    expect(defaultVisionModel(gemini, "gemini-3-pro")).toBe("gemini-3-pro");
  });

  it("labels the footer pill", () => {
    expect(visionLabel("disabled")).toBe("vision off");
    expect(visionLabel("ask", "Gemini", "gemini-2.5-flash")).toBe(
      "vision ask · Gemini · gemini-2.5-flash",
    );
  });

  it("pulls image files off a paste DataTransfer", () => {
    const png = new File([new Uint8Array([1, 2, 3])], "shot.png", { type: "image/png" });
    const txt = new File([new Uint8Array([4])], "notes.txt", { type: "text/plain" });
    const dt = {
      files: [png, txt],
      items: [],
      types: ["Files"],
      getData: () => "",
    } as unknown as DataTransfer;
    expect(imagesFromClipboardEvent(dt).map((f) => f.name)).toEqual(["shot.png"]);
    expect(clipboardLooksLikeImage(dt)).toBe(true);
  });

  it("treats an image MIME type as a screenshot clipboard", () => {
    const dt = {
      files: [],
      items: [],
      types: ["image/png"],
      getData: () => "",
    } as unknown as DataTransfer;
    expect(imagesFromClipboardEvent(dt)).toEqual([]);
    expect(clipboardLooksLikeImage(dt)).toBe(true);
    const text = {
      files: [],
      items: [],
      types: ["text/plain"],
      getData: () => "hello",
    } as unknown as DataTransfer;
    expect(clipboardLooksLikeImage(text)).toBe(false);
  });
});
