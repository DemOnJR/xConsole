export interface ChatSnippet {
  id: string;
  name: string;
  language: string;
  content: string;
  lineCount: number;
  size: number;
}

/**
 * Returns true if text is sufficiently large or looks like code/structured data
 * that should be converted into a snippet attachment rather than raw inline text.
 */
export function shouldCreateSnippet(text: string): boolean {
  if (!text) return false;
  const lines = text.split("\n");
  const lineCount = lines.length;
  const len = text.length;

  // Multi-line block with at least 5 lines
  if (lineCount >= 5) return true;

  // Relatively large text (> 280 chars) with multiple lines
  if (lineCount >= 3 && len >= 280) return true;

  // Single or short multi-line block that is very clearly structured code/data (> 350 chars)
  if (len >= 350 && detectCodeStructure(text)) return true;

  return false;
}

function detectCodeStructure(text: string): boolean {
  const trimmed = text.trim();
  if (trimmed.startsWith("<!DOCTYPE") || trimmed.startsWith("<html") || trimmed.startsWith("<?xml")) return true;
  if ((trimmed.startsWith("{") && trimmed.endsWith("}")) || (trimmed.startsWith("[") && trimmed.endsWith("]"))) {
    try {
      JSON.parse(trimmed);
      return true;
    } catch {
      // ignore
    }
  }
  if (/\b(function|const|let|var|import|export|class|def|public|private|fn|struct|impl|SELECT|INSERT)\b/.test(text)) {
    return true;
  }
  return false;
}

/**
 * Heuristically detect programming language and file extension from code content.
 */
export function detectSnippetLanguage(code: string): { lang: string; ext: string; label: string } {
  const trimmed = code.trim();

  // PHP
  if (
    trimmed.startsWith("<?php") ||
    /\b(public\s+function|private\s+function|\$\w+\s*->|\bnamespace\s+\w+;\b)/.test(code)
  ) {
    return { lang: "php", ext: "php", label: "PHP" };
  }

  // Shell / Bash
  if (
    trimmed.startsWith("#!/") ||
    /\b(sudo\s+|apt-get\s+|curl\s+|docker\s+|docker-compose\s+|npm\s+(install|run|test)|cargo\s+(build|run)|git\s+(checkout|commit|push|pull|status)|chmod\s+|chown\s+)/i.test(code)
  ) {
    return { lang: "sh", ext: "sh", label: "Shell" };
  }

  // JSON
  if (
    (trimmed.startsWith("{") && trimmed.endsWith("}")) ||
    (trimmed.startsWith("[") && trimmed.endsWith("]"))
  ) {
    try {
      JSON.parse(trimmed);
      return { lang: "json", ext: "json", label: "JSON" };
    } catch {
      // not valid JSON, check further
    }
  }

  // TypeScript / TSX
  if (
    /\b(interface\s+\w+|type\s+\w+\s*=|export\s+(default\s+)?(function|const|class)|import\s+.*?\s+from\s+['"]|\buseState<|\buseRef<|:\s*(string|number|boolean|Record<|Promise<|void))\b/.test(code)
  ) {
    if (/<[A-Z]\w+|<\/[A-Z]\w+>|<div|<span|<button/.test(code)) {
      return { lang: "tsx", ext: "tsx", label: "TSX" };
    }
    return { lang: "typescript", ext: "ts", label: "TypeScript" };
  }

  // JavaScript / JSX
  if (
    /\b(const\s+\w+\s*=|let\s+\w+\s*=|var\s+\w+\s*=|function\s*\w*\s*\(|=>\s*\{|require\(|module\.exports|export\s+default|import\s+.*?\s+from\s+['"])\b/.test(code)
  ) {
    if (/<[A-Z]\w+|<\/[A-Z]\w+>|<div|<span|<button/.test(code)) {
      return { lang: "jsx", ext: "jsx", label: "JSX" };
    }
    return { lang: "javascript", ext: "js", label: "JavaScript" };
  }

  // Rust
  if (
    /\b(pub\s+fn|fn\s+\w+\s*\(|impl\s+|use\s+std::|let\s+mut\s+|#\[derive|match\s+\w+\s*\{)\b/.test(code)
  ) {
    return { lang: "rust", ext: "rs", label: "Rust" };
  }

  // Python
  if (
    /\b(def\s+\w+\s*\(|from\s+\w+\s+import|class\s+\w+\s*(\(.*\))?:|elif\s+|if\s+__name__\s*==\s*['"]__main__['"])\b/.test(code) ||
    /^\s*import\s+[a-zA-Z0-9_]+(\s+as\s+[a-zA-Z0-9_]+)?\s*$/m.test(code)
  ) {
    return { lang: "python", ext: "py", label: "Python" };
  }

  // HTML / XML / SVG
  if (
    trimmed.startsWith("<!DOCTYPE") ||
    trimmed.startsWith("<?xml") ||
    /<(html|head|body|div|span|h[1-6]|p|img|table|tr|td|script|style|svg|a|button|form|input)[\s>]/i.test(trimmed)
  ) {
    if (trimmed.startsWith("<svg") || /<svg[\s>]/.test(trimmed)) {
      return { lang: "svg", ext: "svg", label: "SVG" };
    }
    return { lang: "html", ext: "html", label: "HTML" };
  }

  // CSS / SCSS
  if (
    /(@media|@import|@keyframes|\.(?:[a-z0-9_-]+)\s*\{|#(?:[a-z0-9_-]+)\s*\{)\b/i.test(code) ||
    /:\s*(?:flex|grid|block|none|inherit|absolute|relative|pointer|rgba?|hsl|#[\da-f]{3,8});/i.test(code)
  ) {
    return { lang: "css", ext: "css", label: "CSS" };
  }

  // SQL
  if (
    /\b(SELECT\s+.*?\s+FROM|INSERT\s+INTO|UPDATE\s+\w+\s+SET|DELETE\s+FROM|CREATE\s+TABLE|ALTER\s+TABLE|DROP\s+TABLE)\b/i.test(code)
  ) {
    return { lang: "sql", ext: "sql", label: "SQL" };
  }

  // Markdown
  if (/^#{1,6}\s+\w+|\*\*.*?\*\*|\[.*?\]\(.*?\)/m.test(code)) {
    return { lang: "markdown", ext: "md", label: "Markdown" };
  }

  // YAML / TOML
  if (/^[a-zA-Z0-9_-]+:\s*[a-zA-Z0-9_"'-]/m.test(code) && !code.includes("{") && !code.includes("}")) {
    return { lang: "yaml", ext: "yaml", label: "YAML" };
  }

  return { lang: "text", ext: "txt", label: "Code" };
}

/**
 * Creates a ChatSnippet instance with auto-generated ID, detected language, and filename.
 */
export function createChatSnippet(content: string, customName?: string): ChatSnippet {
  const id = `snip-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  const { lang, ext, label } = detectSnippetLanguage(content);
  const lines = content.split("\n").length;
  const name = customName || (label === "Code" || label === "Text" ? `pasted-snippet.${ext}` : `pasted-${lang}.${ext}`);

  return {
    id,
    name,
    language: lang,
    content,
    lineCount: lines,
    size: new Blob([content]).size,
  };
}
