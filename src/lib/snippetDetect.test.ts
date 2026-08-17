import { describe, expect, it } from "vitest";
import {
  createChatSnippet,
  detectSnippetLanguage,
  shouldCreateSnippet,
} from "./snippetDetect";

describe("snippetDetect", () => {
  it("detects when text is small and should remain inline", () => {
    expect(shouldCreateSnippet("hello world")).toBe(false);
    expect(shouldCreateSnippet("npm run dev\nls -la")).toBe(false);
    expect(shouldCreateSnippet("const x = 1;\nconst y = 2;")).toBe(false);
  });

  it("detects when text is multiline code / large text and should become a snippet", () => {
    const htmlCode = `
      <h3><span>The PEGI age labels</span></h3>
      <p>&nbsp;</p>
      <img src="/sites/default/files/inline-images/age-3-black_0.jpg" alt="PEGI 3" />
      <h4>PEGI 3</h4>
      <p>Content of games with a PEGI 3 rating is considered suitable for all age groups.</p>
    `;
    expect(shouldCreateSnippet(htmlCode)).toBe(true);

    const manyLines = "line 1\nline 2\nline 3\nline 4\nline 5";
    expect(shouldCreateSnippet(manyLines)).toBe(true);
  });

  it("correctly identifies language types", () => {
    expect(
      detectSnippetLanguage("<h3><span>The PEGI age labels</span></h3>").lang,
    ).toBe("html");

    expect(
      detectSnippetLanguage("{\"status\": \"ok\", \"count\": 42}").lang,
    ).toBe("json");

    expect(
      detectSnippetLanguage("pub fn calculate_sum(a: i32, b: i32) -> i32 { a + b }").lang,
    ).toBe("rust");

    expect(
      detectSnippetLanguage("def handle_request(req):\n    return req.json()").lang,
    ).toBe("python");

    expect(
      detectSnippetLanguage("import React, { useState } from 'react';\nexport const App = () => <div className='p-4'>Hello</div>;").lang,
    ).toBe("tsx");

    expect(
      detectSnippetLanguage("<?php\npublic function getLayout() { return $this->view; }").lang,
    ).toBe("php");

    expect(
      detectSnippetLanguage("SELECT id, name FROM users WHERE active = 1;").lang,
    ).toBe("sql");
  });

  it("creates a ChatSnippet object with proper metadata", () => {
    const snippet = createChatSnippet("<h3>Hello</h3>\n<p>World</p>\n<div>Test</div>\n<span>End</span>\n<footer>OK</footer>");
    expect(snippet.id).toBeDefined();
    expect(snippet.language).toBe("html");
    expect(snippet.name).toBe("pasted-html.html");
    expect(snippet.lineCount).toBe(5);
    expect(snippet.size).toBeGreaterThan(0);
  });
});
