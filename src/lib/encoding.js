/**
 * Multi-language Character Encoding and Charset Detection utilities.
 * Handles UTF-8, UTF-16, Windows-1250 (Central European / Romanian),
 * Windows-1251 (Cyrillic / Russian), Windows-1252 (Western),
 * Windows-1256 (Arabic), GBK / GB18030 (Chinese), EUC-KR (Korean),
 * Shift-JIS (Japanese), and ISO-8859-*.
 */
export const SUPPORTED_ENCODINGS = [
    { id: "utf-8", name: "UTF-8", category: "Unicode" },
    { id: "utf-8-bom", name: "UTF-8 with BOM", category: "Unicode" },
    { id: "utf-16le", name: "UTF-16 LE", category: "Unicode" },
    { id: "utf-16be", name: "UTF-16 BE", category: "Unicode" },
    { id: "windows-1251", name: "Windows-1251 (Cyrillic / Russian)", category: "Cyrillic" },
    { id: "windows-1250", name: "Windows-1250 (Central European / Romanian)", category: "European" },
    { id: "windows-1252", name: "Windows-1252 (Western European)", category: "European" },
    { id: "windows-1256", name: "Windows-1256 (Arabic)", category: "Middle Eastern" },
    { id: "euc-kr", name: "EUC-KR / CP949 (Korean)", category: "East Asian" },
    { id: "gbk", name: "GBK / GB18030 (Chinese Simplified)", category: "East Asian" },
    { id: "big5", name: "Big5 (Chinese Traditional)", category: "East Asian" },
    { id: "shift_jis", name: "Shift-JIS (Japanese)", category: "East Asian" },
    { id: "iso-8859-1", name: "ISO-8859-1 (Latin-1)", category: "European" },
    { id: "iso-8859-2", name: "ISO-8859-2 (Latin-2)", category: "European" },
];
/** Convert base64 string to Uint8Array safely */
export function base64ToBytes(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i += 1) {
        bytes[i] = bin.charCodeAt(i);
    }
    return bytes;
}
/** Convert Uint8Array to base64 string safely */
export function bytesToBase64(bytes) {
    let bin = "";
    const CHUNK = 0x8000;
    for (let i = 0; i < bytes.length; i += CHUNK) {
        bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
    }
    return btoa(bin);
}
/**
 * Detect character encoding from raw file bytes.
 */
export function detectEncoding(bytes) {
    if (bytes.length === 0)
        return "utf-8";
    // 1. Check for Byte Order Marks (BOM)
    if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
        return "utf-8-bom";
    }
    if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) {
        return "utf-16le";
    }
    if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) {
        return "utf-16be";
    }
    // 2. Strict UTF-8 verification
    let isStrictUtf8 = true;
    try {
        const decoder = new TextDecoder("utf-8", { fatal: true });
        decoder.decode(bytes);
    }
    catch {
        isStrictUtf8 = false;
    }
    if (isStrictUtf8) {
        return "utf-8";
    }
    // 3. Heuristic inspection for non-UTF-8 character sets
    // Count frequency of byte ranges
    let cyrillicScore = 0;
    let centralEuroScore = 0;
    let arabicScore = 0;
    let koreanScore = 0;
    let cjkScore = 0;
    for (let i = 0; i < bytes.length; i += 1) {
        const b = bytes[i];
        // CP1251 Cyrillic letters: 0xC0..0xFF (А..я)
        if (b >= 0xc0 && b <= 0xff)
            cyrillicScore += 1;
        // CP1256 Arabic letters: 0xC1..0xFE
        if (b >= 0xc1 && b <= 0xfe)
            arabicScore += 1;
        // Central European characters
        if ([0xa3, 0xa5, 0xaf, 0xb3, 0xb9, 0xba, 0xde, 0xfe, 0xe3, 0xe2, 0xee].includes(b)) {
            centralEuroScore += 1;
        }
        // 2-byte Korean / EUC-KR sequences (0x81..0xFE followed by 0x41..0xFE)
        if (b >= 0xa1 && b <= 0xfe && i + 1 < bytes.length) {
            const next = bytes[i + 1];
            if (next >= 0xa1 && next <= 0xfe) {
                koreanScore += 2;
                cjkScore += 2;
                i += 1;
            }
        }
    }
    const len = bytes.length;
    if (cyrillicScore / len > 0.05 && cyrillicScore > centralEuroScore) {
        return "windows-1251";
    }
    if (koreanScore / len > 0.08) {
        return "euc-kr";
    }
    if (centralEuroScore / len > 0.03) {
        return "windows-1250";
    }
    if (arabicScore / len > 0.08) {
        return "windows-1256";
    }
    if (cjkScore / len > 0.05) {
        return "gbk";
    }
    // Default fallback for legacy single-byte text
    return "windows-1252";
}
/** Decode raw bytes into Unicode string using the specified encoding. */
export function decodeBytes(bytes, encoding) {
    if (bytes.length === 0)
        return "";
    const norm = encoding.toLowerCase().trim();
    if (norm === "utf-8-bom") {
        const raw = bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf
            ? bytes.subarray(3)
            : bytes;
        return new TextDecoder("utf-8").decode(raw);
    }
    try {
        const decoder = new TextDecoder(norm);
        return decoder.decode(bytes);
    }
    catch {
        // Fallback to UTF-8
        return new TextDecoder("utf-8").decode(bytes);
    }
}
// Single-byte code-page mapping tables for encoding back
// Windows-1251 (Cyrillic) unicode mappings (0x80 - 0xFF)
const CP1251_MAP = {
    "\u0402": 0x80, "\u0403": 0x81, "\u201a": 0x82, "\u0453": 0x83, "\u201e": 0x84, "\u2026": 0x85, "\u2020": 0x86, "\u2021": 0x87,
    "\u20ac": 0x88, "\u2030": 0x89, "\u0409": 0x8a, "\u2039": 0x8b, "\u040a": 0x8c, "\u040c": 0x8d, "\u040b": 0x8e, "\u040f": 0x8f,
    "\u0452": 0x90, "\u2018": 0x91, "\u2019": 0x92, "\u201c": 0x93, "\u201d": 0x94, "\u2022": 0x95, "\u2013": 0x96, "\u2014": 0x97,
    "\u2122": 0x99, "\u0459": 0x9a, "\u203a": 0x9b, "\u045a": 0x9c, "\u045c": 0x9d, "\u045b": 0x9e, "\u045f": 0x9f,
    "\u00a0": 0xa0, "\u040e": 0xa1, "\u045e": 0xa2, "\u0408": 0xa3, "\u00a4": 0xa4, "\u0490": 0xa5, "\u00a6": 0xa6, "\u00a7": 0xa7,
    "\u0401": 0xa8, "\u00a9": 0xa9, "\u0404": 0xaa, "\u00ab": 0xab, "\u00ac": 0xac, "\u00ad": 0xad, "\u00ae": 0xae, "\u0407": 0xaf,
    "\u00b0": 0xb0, "\u00b1": 0xb1, "\u0406": 0xb2, "\u0456": 0xb3, "\u0491": 0xb4, "\u00b5": 0xb5, "\u00b6": 0xb6, "\u00b7": 0xb7,
    "\u0451": 0xb8, "\u2116": 0xb9, "\u0454": 0xba, "\u00bb": 0xbb, "\u0458": 0xbc, "\u0405": 0xbd, "\u0455": 0xbe, "\u0457": 0xbf,
};
// Add standard Cyrillic А..я range (0xC0..0xFF)
for (let code = 0x0410; code <= 0x044f; code += 1) {
    const ch = String.fromCharCode(code);
    CP1251_MAP[ch] = code - 0x0410 + 0xc0;
}
// Windows-1250 (Central European / Romanian) mappings (0x80 - 0xFF)
const CP1250_MAP = {
    "\u20ac": 0x80, "\u201a": 0x82, "\u201e": 0x84, "\u2026": 0x85, "\u2020": 0x86, "\u2021": 0x87, "\u2030": 0x89,
    "\u0160": 0x8a, "\u2039": 0x8b, "\u015a": 0x8c, "\u0164": 0x8d, "\u017d": 0x8e, "\u0179": 0x8f,
    "\u2018": 0x91, "\u2019": 0x92, "\u201c": 0x93, "\u201d": 0x94, "\u2022": 0x95, "\u2013": 0x96, "\u2014": 0x97,
    "\u2122": 0x99, "\u0161": 0x9a, "\u203a": 0x9b, "\u015b": 0x9c, "\u0165": 0x9d, "\u017e": 0x9e, "\u017a": 0x9f,
    "\u00a0": 0xa0, "\u02c7": 0xa1, "\u02d8": 0xa2, "\u0141": 0xa3, "\u00a4": 0xa4, "\u0104": 0xa5, "\u00a6": 0xa6, "\u00a7": 0xa7,
    "\u00a8": 0xa8, "\u00a9": 0xa9, "\u015e": 0xaa, "\u00ab": 0xab, "\u00ac": 0xac, "\u00ad": 0xad, "\u00ae": 0xae, "\u017b": 0xaf,
    "\u00b0": 0xb0, "\u00b1": 0xb1, "\u02db": 0xb2, "\u0142": 0xb3, "\u00b4": 0xb4, "\u00b5": 0xb5, "\u00b6": 0xb6, "\u00b7": 0xb7,
    "\u00b8": 0xb8, "\u0105": 0xb9, "\u015f": 0xba, "\u00bb": 0xbb, "\u013d": 0xbc, "\u02dd": 0xbd, "\u013e": 0xbe, "\u017c": 0xbf,
    "\u0154": 0xc0, "\u00c1": 0xc1, "\u00c2": 0xc2, "\u0102": 0xc3, "\u00c4": 0xc4, "\u0139": 0xc5, "\u0106": 0xc6, "\u00c7": 0xc7,
    "\u010c": 0xc8, "\u00c9": 0xc9, "\u0118": 0xca, "\u00cb": 0xcb, "\u011a": 0xcc, "\u00cd": 0xcd, "\u00ce": 0xce, "\u010e": 0xcf,
    "\u0110": 0xd0, "\u0143": 0xd1, "\u0147": 0xd2, "\u00d3": 0xd3, "\u00d4": 0xd4, "\u0150": 0xd5, "\u00d6": 0xd6, "\u00d7": 0xd7,
    "\u0158": 0xd8, "\u016e": 0xd9, "\u00da": 0xda, "\u0170": 0xdb, "\u00dc": 0xdc, "\u00dd": 0xdd, "\u0162": 0xde, "\u00df": 0xdf,
    "\u0155": 0xe0, "\u00e1": 0xe1, "\u00e2": 0xe2, "\u0103": 0xe3, "\u00e4": 0xe4, "\u013a": 0xe5, "\u0107": 0xe6, "\u00e7": 0xe7,
    "\u010d": 0xe8, "\u00e9": 0xe9, "\u0119": 0xea, "\u00eb": 0xeb, "\u011b": 0xec, "\u00ed": 0xed, "\u00ee": 0xee, "\u010f": 0xef,
    "\u0111": 0xf0, "\u0144": 0xf1, "\u0148": 0xf2, "\u00f3": 0xf3, "\u00f4": 0xf4, "\u0151": 0xf5, "\u00f6": 0xf6, "\u00f7": 0xf7,
    "\u0159": 0xf8, "\u016f": 0xf9, "\u00fa": 0xfa, "\u0171": 0xfb, "\u00fc": 0xfc, "\u00fd": 0xfd, "\u0163": 0xfe, "\u02d9": 0xff,
    // Romanian comma-below variants mapping to cedilla for CP1250 compatibility
    "\u0218": 0xaa, "\u0219": 0xba, "\u021a": 0xde, "\u021b": 0xfe,
};
/** Encode text into the specified encoding bytes. */
export function encodeText(text, encoding) {
    const norm = encoding.toLowerCase().trim();
    if (norm === "utf-8") {
        return new TextEncoder().encode(text);
    }
    if (norm === "utf-8-bom") {
        const raw = new TextEncoder().encode(text);
        const out = new Uint8Array(raw.length + 3);
        out[0] = 0xef;
        out[1] = 0xbb;
        out[2] = 0xbf;
        out.set(raw, 3);
        return out;
    }
    if (norm === "utf-16le") {
        const out = new Uint8Array(text.length * 2);
        for (let i = 0; i < text.length; i += 1) {
            const code = text.charCodeAt(i);
            out[i * 2] = code & 0xff;
            out[i * 2 + 1] = (code >> 8) & 0xff;
        }
        return out;
    }
    if (norm === "utf-16be") {
        const out = new Uint8Array(text.length * 2);
        for (let i = 0; i < text.length; i += 1) {
            const code = text.charCodeAt(i);
            out[i * 2] = (code >> 8) & 0xff;
            out[i * 2 + 1] = code & 0xff;
        }
        return out;
    }
    // Windows-1251 encoder
    if (norm === "windows-1251") {
        const out = new Uint8Array(text.length);
        for (let i = 0; i < text.length; i += 1) {
            const ch = text[i];
            const code = ch.charCodeAt(0);
            if (code < 0x80) {
                out[i] = code;
            }
            else if (CP1251_MAP[ch] !== undefined) {
                out[i] = CP1251_MAP[ch];
            }
            else {
                out[i] = 0x3f; // '?'
            }
        }
        return out;
    }
    // Windows-1250 encoder
    if (norm === "windows-1250") {
        const out = new Uint8Array(text.length);
        for (let i = 0; i < text.length; i += 1) {
            const ch = text[i];
            const code = ch.charCodeAt(0);
            if (code < 0x80) {
                out[i] = code;
            }
            else if (CP1250_MAP[ch] !== undefined) {
                out[i] = CP1250_MAP[ch];
            }
            else {
                out[i] = 0x3f; // '?'
            }
        }
        return out;
    }
    // Default ISO-8859-1 / Windows-1252 standard single-byte encoder
    const out = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i += 1) {
        const code = text.charCodeAt(i);
        out[i] = code <= 0xff ? code : 0x3f;
    }
    return out;
}
/** Check whether converting text to the target encoding would lose/corrupt any characters. */
export function checkEncodingLoss(text, targetEncoding) {
    const norm = targetEncoding.toLowerCase().trim();
    if (norm === "utf-8" || norm === "utf-8-bom" || norm === "utf-16le" || norm === "utf-16be") {
        return { hasLoss: false, lostChars: [] };
    }
    const encoded = encodeText(text, targetEncoding);
    const decoded = decodeBytes(encoded, targetEncoding);
    const lost = new Set();
    for (let i = 0; i < text.length; i += 1) {
        if (text[i] !== decoded[i]) {
            lost.add(text[i]);
        }
    }
    return {
        hasLoss: lost.size > 0,
        lostChars: Array.from(lost).slice(0, 10),
    };
}
//# sourceMappingURL=encoding.js.map