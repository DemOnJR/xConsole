/** Resolve a `cd` argument against a known working directory. */
export function resolveCd(cwd: string, arg: string): string {
  const trimmed = arg.trim();
  if (!trimmed || trimmed === "~") return cwd;
  if (trimmed.startsWith("/")) {
    return normalizePath(trimmed);
  }
  if (trimmed === "..") {
    const p = cwd.replace(/\/+$/, "") || "/";
    if (p === "/") return "/";
    const idx = p.lastIndexOf("/");
    return idx <= 0 ? "/" : p.slice(0, idx);
  }
  if (trimmed === ".") return cwd;
  const base = cwd.endsWith("/") ? cwd.slice(0, -1) : cwd;
  return normalizePath(`${base}/${trimmed}`);
}

export function normalizePath(path: string): string {
  const parts = path.split("/").filter(Boolean);
  const out: string[] = [];
  for (const part of parts) {
    if (part === ".") continue;
    if (part === "..") {
      out.pop();
      continue;
    }
    out.push(part);
  }
  return `/${out.join("/")}` || "/";
}

/** Extract cwd updates from terminal output (OSC 7, VS Code shell integration). */
export function extractCwdFromOutput(chunk: string): string | null {
  let found: string | null = null;

  // OSC 7: \x1b]7;file://host/path\x07 or \x1b]7;file://host/path\x1b\\
  const osc7 =
    /\x1b\]7;file:\/\/[^/]*(\/[^\x07\x1b]*)(?:\x07|\x1b\\)/g;
  for (const m of chunk.matchAll(osc7)) {
    try {
      found = normalizePath(decodeURIComponent(m[1]));
    } catch {
      found = normalizePath(m[1]);
    }
  }

  // VS Code / shell integration: \x1b]633;P;Cwd=/path\x07
  const osc633 = /\x1b\]633;P;Cwd=([^\x07\x1b]+)(?:\x07|\x1b\\)/g;
  for (const m of chunk.matchAll(osc633)) {
    found = normalizePath(m[1]);
  }

  return found;
}

/** Detect a completed `cd` command from terminal input. */
export function cwdFromCdInput(line: string, cwd: string | undefined): string | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("cd")) return null;
  const rest = trimmed.slice(2).trim();
  if (!rest) return null; // cd with no args → home; wait for OSC 7
  const unquoted = rest.replace(/^['"]|['"]$/g, "");
  return resolveCd(cwd ?? "/", unquoted);
}
