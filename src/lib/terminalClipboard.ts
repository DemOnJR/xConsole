import {
  readText,
  writeText,
  readImage,
} from "@tauri-apps/plugin-clipboard-manager";

/**
 * Clipboard access for terminals.
 *
 * Goes through the Tauri plugin rather than `navigator.clipboard` because the webview
 * refuses programmatic *reads* without a user-gesture heuristic we cannot rely on (the
 * Ctrl+right-click paste has no keyboard event at all), and cannot see image data in any
 * form — which is the whole point of pasting a screenshot into a terminal.
 */

export async function copyToClipboard(text: string): Promise<void> {
  await writeText(text);
}

export async function pasteFromClipboard(): Promise<string> {
  try {
    return await readText();
  } catch {
    // An image-only clipboard has no text; that is not an error worth surfacing here.
    return "";
  }
}

/** PNG bytes on the clipboard, or null if it holds no image. */
export async function clipboardImagePng(): Promise<Uint8Array | null> {
  try {
    const img = await readImage();
    const bytes = await img.rgba();
    if (!bytes?.length) return null;
    // The plugin hands back raw RGBA, so it has to be encoded before it is a file
    // anyone can open. Done on a canvas: no image encoder in the dependency tree, and
    // a screenshot is exactly the case where a real PNG matters.
    const size = await img.size();
    return await rgbaToPng(bytes, size.width, size.height);
  } catch {
    return null;
  }
}

async function rgbaToPng(
  rgba: Uint8Array,
  width: number,
  height: number,
): Promise<Uint8Array | null> {
  if (width <= 0 || height <= 0) return null;
  const expected = width * height * 4;
  let pixels: Uint8ClampedArray;
  if (rgba.length === expected) {
    pixels = new Uint8ClampedArray(rgba);
  } else if (rgba.length > expected) {
    pixels = new Uint8ClampedArray(rgba.subarray(0, expected));
  } else {
    return null;
  }
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.putImageData(new ImageData(pixels, width, height), 0, 0);
  const blob = await new Promise<Blob | null>((res) =>
    canvas.toBlob((b) => res(b), "image/png"),
  );
  if (!blob) return null;
  return new Uint8Array(await blob.arrayBuffer());
}

/**
 * Quote a path for a POSIX shell.
 *
 * Single quotes, with embedded single quotes closed-escaped-reopened. Dropped filenames
 * are attacker-adjacent input in the sense that matters here: a screenshot named
 * `; rm -rf ~ #.png` is a perfectly legal filename, and this text is typed straight into
 * a live root shell. Note the doubled backslash — `"'\\''"` is the four characters
 * `'\''`; writing `"'\''"` produces `'''` and reopens the quote in the wrong place.
 */
export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}
