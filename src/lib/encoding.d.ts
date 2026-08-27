/**
 * Multi-language Character Encoding and Charset Detection utilities.
 * Handles UTF-8, UTF-16, Windows-1250 (Central European / Romanian),
 * Windows-1251 (Cyrillic / Russian), Windows-1252 (Western),
 * Windows-1256 (Arabic), GBK / GB18030 (Chinese), EUC-KR (Korean),
 * Shift-JIS (Japanese), and ISO-8859-*.
 */
export interface EncodingOption {
    id: string;
    name: string;
    category: "Unicode" | "Cyrillic" | "East Asian" | "Middle Eastern" | "European";
}
export declare const SUPPORTED_ENCODINGS: EncodingOption[];
/** Convert base64 string to Uint8Array safely */
export declare function base64ToBytes(b64: string): Uint8Array;
/** Convert Uint8Array to base64 string safely */
export declare function bytesToBase64(bytes: Uint8Array): string;
/**
 * Detect character encoding from raw file bytes.
 */
export declare function detectEncoding(bytes: Uint8Array): string;
/** Decode raw bytes into Unicode string using the specified encoding. */
export declare function decodeBytes(bytes: Uint8Array, encoding: string): string;
/** Encode text into the specified encoding bytes. */
export declare function encodeText(text: string, encoding: string): Uint8Array;
/** Check whether converting text to the target encoding would lose/corrupt any characters. */
export declare function checkEncodingLoss(text: string, targetEncoding: string): {
    hasLoss: boolean;
    lostChars: string[];
};
//# sourceMappingURL=encoding.d.ts.map