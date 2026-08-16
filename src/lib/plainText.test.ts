import { describe, expect, it } from "vitest";
import { plainText } from "./plainText";

describe("plainText", () => {
  it("strips markdown formatting", () => {
    expect(plainText("**bold** and *italic* and `code`")).toBe("bold and italic and code");
  });

  it("keeps code fence contents, drops the fence markers", () => {
    expect(plainText("```ts\nconst x = 1;\n```")).toBe("const x = 1;");
  });

  it("turns links into their text", () => {
    expect(plainText("[click me](https://example.com)")).toBe("click me");
    expect(plainText("![alt](img.png)")).toBe("alt");
  });

  it("strips headings, blockquotes, and HTML", () => {
    expect(plainText("# Title\n\n> quote\n\n<div>hi</div>")).toBe("Title\n\nquote\n\nhi");
  });

  it("normalizes list markers", () => {
    expect(plainText("- one\n- two\n3. three")).toBe("- one\n- two\n- three");
  });

  it("does not leak raw URLs or tags", () => {
    const out = plainText("<script>alert(1)</script> [x](javascript:alert(1))");
    expect(out).not.toContain("<");
    expect(out).not.toContain("javascript:");
  });
});
