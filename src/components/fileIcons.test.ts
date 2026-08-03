import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  ArchiveIcon,
  DatabaseFileIcon,
  DocIcon,
  ImageFileIcon,
  KeyFileIcon,
  PlainFileIcon,
  ShellIcon,
  SymlinkIcon,
  fileKindFor,
} from "./fileIcons";
import { FolderIcon } from "./icons";

const file = (name: string) => ({ name, is_dir: false });

/** The letters a lettered sheet puts in its band, read straight off the rendered SVG. */
function renderTag(name: string): string {
  const { Icon } = fileKindFor(file(name));
  const markup = renderToStaticMarkup(createElement(Icon));
  return /<text[^>]*>([^<]*)<\/text>/.exec(markup)?.[1] ?? "";
}

describe("fileKindFor", () => {
  it("draws the shape of things that have one", () => {
    expect(fileKindFor(file("backup.tar.gz")).Icon).toBe(ArchiveIcon);
    expect(fileKindFor(file("logo.png")).Icon).toBe(ImageFileIcon);
    expect(fileKindFor(file("id_rsa")).Icon).toBe(KeyFileIcon);
    expect(fileKindFor(file("data.sqlite")).Icon).toBe(DatabaseFileIcon);
    expect(fileKindFor(file("deploy.sh")).Icon).toBe(ShellIcon);
    expect(fileKindFor(file("notes.md")).Icon).toBe(DocIcon);
  });

  /// The point of the lettered sheets: a text format is identified by its name, so every
  /// language gets its own icon rather than forty sharing one glyph and a tint.
  it("gives every text format its own icon, not a shared one", () => {
    const icons = [
      "index.php",
      "main.go",
      "main.rs",
      "app.ts",
      "app.tsx",
      "style.scss",
      "config.yml",
      "data.json",
      "page.html",
    ].map((n) => fileKindFor(file(n)).Icon);
    expect(new Set(icons).size).toBe(icons.length);
    // ...and none of them is the generic sheet.
    expect(icons).not.toContain(PlainFileIcon);
  });

  /// The band carries the extension itself, so what is rendered is predictable from the
  /// filename — except where the raw extension reads wrong.
  it("labels a sheet with the format's own name", () => {
    expect(renderTag("index.php")).toBe("PHP");
    expect(renderTag("app.tsx")).toBe("TSX");
    expect(renderTag("config.yml")).toBe("YML");
    expect(renderTag("main.cpp")).toBe("C++");
    expect(renderTag("Program.cs")).toBe("C#");
    // Long extensions are cut rather than allowed to overflow the band.
    expect(renderTag("a.properties").length).toBeLessThanOrEqual(4);
  });

  /// What a thing *is* outranks what it is called: a directory named like a stylesheet is
  /// still a directory, and opening it is what the icon has to predict.
  it("puts type before name", () => {
    expect(fileKindFor({ name: "styles.css", is_dir: true }).Icon).toBe(FolderIcon);
    expect(
      fileKindFor({ name: "app.php", is_dir: false, is_symlink: true }).Icon,
    ).toBe(SymlinkIcon);
    // A link to a directory is still a link — that is the useful fact about it.
    expect(fileKindFor({ name: "current", is_dir: true, is_symlink: true }).Icon).toBe(
      SymlinkIcon,
    );
  });

  /// A broken link has to be visibly different from a working one; in a plain listing they
  /// are identical, and it is usually the thing being hunted.
  it("marks a broken link apart from a working one", () => {
    const broken = fileKindFor({
      name: "current",
      is_dir: false,
      is_symlink: true,
      link_broken: true,
    });
    const working = fileKindFor({ name: "current", is_dir: false, is_symlink: true });
    expect(broken.className).not.toBe(working.className);
    expect(broken.label).toMatch(/broken/i);
  });

  /// Servers are full of files with no extension at all, and they are exactly the ones
  /// someone browsing a server is looking for.
  it("knows files that have no extension", () => {
    expect(fileKindFor(file("Dockerfile")).label).toBe("Dockerfile");
    expect(fileKindFor(file("Makefile")).label).toBe("Makefile");
    expect(fileKindFor(file("authorized_keys")).Icon).toBe(KeyFileIcon);
    // Case does not matter: DOCKERFILE and dockerfile are both seen in the wild.
    expect(fileKindFor(file("DOCKERFILE")).label).toBe("Dockerfile");
  });

  /// A leading dot names the file, it does not introduce an extension — otherwise `.env`
  /// would be classified by "env" as a suffix of nothing.
  it("treats a dotfile as a name, not an extension", () => {
    expect(fileKindFor(file(".gitignore")).label).toMatch(/git/i);
    expect(fileKindFor(file(".bashrc")).Icon).toBe(ShellIcon);
    expect(fileKindFor(file(".env")).label).toMatch(/environment/i);
  });

  /// The stem is a fallback, so `README.md` still reads as a readme — but only after the
  /// extension has had its say, or `license.js` would stop being JavaScript.
  it("prefers the extension over the stem", () => {
    expect(renderTag("license.js")).toBe("JS");
    expect(fileKindFor(file("README.md")).Icon).toBe(DocIcon);
    expect(fileKindFor(file("README")).label).toBe("Readme");
  });

  it("falls back to a plain sheet, and never throws", () => {
    expect(fileKindFor(file("mystery.qqq")).Icon).toBe(PlainFileIcon);
    expect(fileKindFor(file("")).Icon).toBe(PlainFileIcon);
    expect(fileKindFor(file("trailing.")).Icon).toBe(PlainFileIcon);
    expect(fileKindFor(file(".")).Icon).toBe(PlainFileIcon);
  });

  it("always offers a label to hover, whatever the file", () => {
    for (const n of ["a.php", "b.zip", "weird.qqq", "Dockerfile", ".env", ""]) {
      expect(fileKindFor(file(n)).label.length).toBeGreaterThan(0);
    }
  });
});
