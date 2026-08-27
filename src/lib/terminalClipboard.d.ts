/**
 * Clipboard access for terminals.
 *
 * Goes through the Tauri plugin rather than `navigator.clipboard` because the webview
 * refuses programmatic *reads* without a user-gesture heuristic we cannot rely on (the
 * Ctrl+right-click paste has no keyboard event at all), and cannot see image data in any
 * form — which is the whole point of pasting a screenshot into a terminal.
 */
export declare function copyToClipboard(text: string): Promise<void>;
export declare function pasteFromClipboard(): Promise<string>;
/** PNG bytes on the clipboard, or null if it holds no image. */
export declare function clipboardImagePng(): Promise<Uint8Array | null>;
/**
 * Quote a path for a POSIX shell.
 *
 * Single quotes, with embedded single quotes closed-escaped-reopened. Dropped filenames
 * are attacker-adjacent input in the sense that matters here: a screenshot named
 * `; rm -rf ~ #.png` is a perfectly legal filename, and this text is typed straight into
 * a live root shell. Note the doubled backslash — `"'\\''"` is the four characters
 * `'\''`; writing `"'\''"` produces `'''` and reopens the quote in the wrong place.
 */
export declare function shellQuote(s: string): string;
//# sourceMappingURL=terminalClipboard.d.ts.map