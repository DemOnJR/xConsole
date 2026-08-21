import { describe, expect, it } from "vitest";
import {
  base64ToBytes,
  bytesToBase64,
  checkEncodingLoss,
  decodeBytes,
  detectEncoding,
  encodeText,
} from "./encoding";

describe("encoding utilities", () => {
  it("detects UTF-8 BOM, UTF-16LE, UTF-16BE", () => {
    expect(detectEncoding(new Uint8Array([0xef, 0xbb, 0xbf, 0x61, 0x62]))).toBe("utf-8-bom");
    expect(detectEncoding(new Uint8Array([0xff, 0xfe, 0x61, 0x00]))).toBe("utf-16le");
    expect(detectEncoding(new Uint8Array([0xfe, 0xff, 0x00, 0x61]))).toBe("utf-16be");
  });

  it("detects standard UTF-8 text with multi-byte characters", () => {
    const text = "Salut, lume! Привет мир! 这是一个测试";
    const bytes = new TextEncoder().encode(text);
    expect(detectEncoding(bytes)).toBe("utf-8");
    expect(decodeBytes(bytes, "utf-8")).toBe(text);
  });

  it("detects and decodes Windows-1251 Cyrillic", () => {
    // "Привет" in Windows-1251: CF F0 E8 E2 E5 F2
    const bytes = new Uint8Array([0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2]);
    expect(detectEncoding(bytes)).toBe("windows-1251");
    expect(decodeBytes(bytes, "windows-1251")).toBe("Привет");
  });

  it("encodes Windows-1251 Cyrillic accurately", () => {
    const text = "Привет";
    const encoded = encodeText(text, "windows-1251");
    expect(Array.from(encoded)).toEqual([0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2]);
  });

  it("detects and encodes Windows-1250 Romanian accents", () => {
    const text = "Mâncare și băutură";
    const encoded = encodeText(text, "windows-1250");
    const decoded = decodeBytes(encoded, "windows-1250");
    expect(decoded).toContain("ncare");
  });

  it("detects character loss when saving into incompatible encoding", () => {
    const cyrillicText = "Привет мир";
    const lossCheck = checkEncodingLoss(cyrillicText, "windows-1252");
    expect(lossCheck.hasLoss).toBe(true);
    expect(lossCheck.lostChars.length).toBeGreaterThan(0);

    const safeCheck = checkEncodingLoss(cyrillicText, "windows-1251");
    expect(safeCheck.hasLoss).toBe(false);
  });

  it("round-trips base64 conversion", () => {
    const original = new Uint8Array([1, 2, 3, 200, 250, 0, 15]);
    const b64 = bytesToBase64(original);
    const roundTrip = base64ToBytes(b64);
    expect(Array.from(roundTrip)).toEqual(Array.from(original));
  });
});
