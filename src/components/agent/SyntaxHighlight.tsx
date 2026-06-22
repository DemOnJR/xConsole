import { useMemo } from "react";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import shell from "highlight.js/lib/languages/shell";
import typescript from "highlight.js/lib/languages/typescript";
import javascript from "highlight.js/lib/languages/javascript";
import rust from "highlight.js/lib/languages/rust";
import python from "highlight.js/lib/languages/python";
import json from "highlight.js/lib/languages/json";
import yaml from "highlight.js/lib/languages/yaml";
import xml from "highlight.js/lib/languages/xml";
import nginx from "highlight.js/lib/languages/nginx";
import cpp from "highlight.js/lib/languages/cpp";
import c from "highlight.js/lib/languages/c";
import php from "highlight.js/lib/languages/php";
import java from "highlight.js/lib/languages/java";
import go from "highlight.js/lib/languages/go";
import csharp from "highlight.js/lib/languages/csharp";
import ruby from "highlight.js/lib/languages/ruby";
import sql from "highlight.js/lib/languages/sql";
import css from "highlight.js/lib/languages/css";
import scss from "highlight.js/lib/languages/scss";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import markdown from "highlight.js/lib/languages/markdown";
import ini from "highlight.js/lib/languages/ini";
import kotlin from "highlight.js/lib/languages/kotlin";
import swift from "highlight.js/lib/languages/swift";
import lua from "highlight.js/lib/languages/lua";
import perl from "highlight.js/lib/languages/perl";
import powershell from "highlight.js/lib/languages/powershell";

const REGISTERED: [string, Parameters<typeof hljs.registerLanguage>[1]][] = [
  ["bash", bash],
  ["shell", shell],
  ["typescript", typescript],
  ["javascript", javascript],
  ["rust", rust],
  ["python", python],
  ["json", json],
  ["yaml", yaml],
  ["xml", xml],
  ["html", xml],
  ["nginx", nginx],
  ["cpp", cpp],
  ["c", c],
  ["php", php],
  ["java", java],
  ["go", go],
  ["csharp", csharp],
  ["ruby", ruby],
  ["sql", sql],
  ["css", css],
  ["scss", scss],
  ["dockerfile", dockerfile],
  ["markdown", markdown],
  ["ini", ini],
  ["kotlin", kotlin],
  ["swift", swift],
  ["lua", lua],
  ["perl", perl],
  ["powershell", powershell],
];

for (const [name, lang] of REGISTERED) {
  hljs.registerLanguage(name, lang);
}

/** Map common fence / alias names to a registered highlight.js language id. */
export function normalizeLanguage(raw?: string): string | undefined {
  if (!raw) return undefined;
  const lang = raw.trim().toLowerCase();
  if (!lang || lang === "text" || lang === "txt" || lang === "plain" || lang === "plaintext") {
    return undefined;
  }

  const ALIASES: Record<string, string> = {
    "c++": "cpp",
    cc: "cpp",
    cxx: "cpp",
    hpp: "cpp",
    h: "cpp",
    cs: "csharp",
    "c#": "csharp",
    js: "javascript",
    jsx: "javascript",
    mjs: "javascript",
    cjs: "javascript",
    ts: "typescript",
    tsx: "typescript",
    py: "python",
    rb: "ruby",
    sh: "bash",
    zsh: "bash",
    shellsession: "bash",
    console: "bash",
    yml: "yaml",
    htm: "html",
    svg: "xml",
    docker: "dockerfile",
    md: "markdown",
    mdx: "markdown",
    ps1: "powershell",
    pwsh: "powershell",
    golang: "go",
    kt: "kotlin",
    kts: "kotlin",
    rs: "rust",
    php3: "php",
    php4: "php",
    php5: "php",
    php7: "php",
    php8: "php",
  };

  const mapped = ALIASES[lang] ?? lang;
  return hljs.getLanguage(mapped) ? mapped : undefined;
}

/** Extract language from react-markdown `className` (e.g. `language-python`). */
export function langFromMarkdownClass(className?: string): string | undefined {
  if (!className) return undefined;
  const match = className.match(/(?:^|\s)language-([\w+#.-]+)/i);
  return normalizeLanguage(match?.[1]);
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function highlightCode(code: string, language?: string): string {
  const trimmed = code.replace(/\r\n/g, "\n");
  if (!trimmed.trim()) return "";

  const lang = normalizeLanguage(language);
  try {
    if (lang) {
      return hljs.highlight(trimmed, { language: lang, ignoreIllegals: true }).value;
    }
  } catch {
    /* fall through */
  }
  try {
    return hljs.highlightAuto(trimmed).value;
  } catch {
    return escapeHtml(trimmed);
  }
}

export function langFromPath(path: string): string | undefined {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
      return "javascript";
    case "rs":
      return "rust";
    case "py":
    case "pyw":
      return "python";
    case "json":
      return "json";
    case "yml":
    case "yaml":
      return "yaml";
    case "html":
    case "htm":
    case "svg":
      return "html";
    case "cpp":
    case "cc":
    case "cxx":
    case "hpp":
    case "hh":
      return "cpp";
    case "h":
      return "cpp";
    case "c":
      return "c";
    case "php":
      return "php";
    case "java":
      return "java";
    case "go":
      return "go";
    case "cs":
      return "csharp";
    case "rb":
      return "ruby";
    case "sql":
      return "sql";
    case "css":
      return "css";
    case "scss":
      return "scss";
    case "dockerfile":
      return "dockerfile";
    case "md":
    case "mdx":
      return "markdown";
    case "ini":
    case "toml":
      return "ini";
    case "kt":
    case "kts":
      return "kotlin";
    case "swift":
      return "swift";
    case "lua":
      return "lua";
    case "pl":
    case "pm":
      return "perl";
    case "ps1":
      return "powershell";
    case "sh":
    case "bash":
    case "zsh":
      return "bash";
    case "conf":
      return path.includes("nginx") ? "nginx" : undefined;
    default:
      return undefined;
  }
}

/** Bash / shell command with syntax colors. */
export function ShellCommand({
  code,
  className = "",
}: {
  code: string;
  className?: string;
}) {
  const html = useMemo(() => highlightCode(code, "bash"), [code]);
  if (!html) return null;

  return (
    <pre className={`agent-hl m-0 overflow-x-auto ${className}`}>
      <code
        className="hljs language-bash !bg-transparent !p-0 text-[10px] leading-relaxed"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </pre>
  );
}

/** Source code block (markdown fences, file diffs, etc.). */
export function CodeHighlight({
  code,
  language,
  className = "",
}: {
  code: string;
  language?: string;
  className?: string;
}) {
  const lang = normalizeLanguage(language);
  const html = useMemo(() => highlightCode(code, lang), [code, lang]);
  if (!html) return null;

  const langClass = lang ? `language-${lang}` : "";

  return (
    <code
      className={`hljs agent-hl-code !bg-transparent !p-0 ${langClass} ${className}`.trim()}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

/** Fenced code block for agent markdown replies. */
export function MarkdownCodeBlock({
  code,
  className,
}: {
  code: string;
  className?: string;
}) {
  const lang = langFromMarkdownClass(className);
  const label = lang ?? "code";

  return (
    <div className="group relative my-2 overflow-hidden rounded-md border border-[#1f2737] bg-[#070a10]">
      <div className="flex items-center border-b border-[#1f2737]/80 px-3 py-1">
        <span className="font-mono text-[10px] uppercase tracking-wide text-gray-500">
          {label}
        </span>
      </div>
      <pre className="agent-hl m-0 overflow-x-auto px-3 py-2">
        <CodeHighlight
          code={code}
          language={lang}
          className="block font-mono text-[12px] leading-relaxed"
        />
      </pre>
    </div>
  );
}

const ANSI_RESET = 0;
const ANSI_MAP: Record<number, string> = {
  1: "ansi-bold",
  30: "ansi-black",
  31: "ansi-red",
  32: "ansi-green",
  33: "ansi-yellow",
  34: "ansi-blue",
  35: "ansi-magenta",
  36: "ansi-cyan",
  37: "ansi-white",
  90: "ansi-bright-black",
  91: "ansi-bright-red",
  92: "ansi-bright-green",
  93: "ansi-bright-yellow",
  94: "ansi-bright-blue",
  95: "ansi-bright-magenta",
  96: "ansi-bright-cyan",
  97: "ansi-bright-white",
};

function ansiClasses(codes: number[]): string {
  if (codes.length === 0 || codes.includes(ANSI_RESET)) return "ansi-default";
  return codes.map((c) => ANSI_MAP[c] ?? "").filter(Boolean).join(" ") || "ansi-default";
}

function parseAnsiSegments(text: string): { className: string; text: string }[] {
  const segments: { className: string; text: string }[] = [];
  let codes: number[] = [];
  let buf = "";
  let i = 0;

  const flush = () => {
    if (!buf) return;
    segments.push({ className: ansiClasses(codes), text: buf });
    buf = "";
  };

  while (i < text.length) {
    if (text[i] === "\x1b" && text[i + 1] === "[") {
      flush();
      i += 2;
      let seq = "";
      while (i < text.length && text[i] !== "m") {
        seq += text[i];
        i += 1;
      }
      if (text[i] === "m") i += 1;
      const parts = seq.split(";").filter(Boolean);
      if (parts.length === 0 || seq === "") {
        codes = [];
      } else {
        for (const p of parts) {
          const n = Number.parseInt(p, 10);
          if (!Number.isNaN(n)) {
            if (n === ANSI_RESET) codes = [];
            else codes.push(n);
          }
        }
      }
      continue;
    }
    buf += text[i];
    i += 1;
  }
  flush();
  return segments;
}

function logLineClass(line: string): string {
  if (/KCPU|error|failed|fatal|denied|cannot|panic|exception|traceback/i.test(line)) {
    return "log-error";
  }
  if (/warn|warning|deprecated/i.test(line)) {
    return "log-warn";
  }
  if (/^\s*(info|notice|ok|success|done|active|running)\b/i.test(line)) {
    return "log-info";
  }
  return "log-default";
}

/** Terminal / SSH output — ANSI colors + log-level line tinting. */
export function ConsoleOutput({
  text,
  className = "",
}: {
  text: string;
  className?: string;
}) {
  const body = useMemo(() => {
    const raw = text.replace(/\r\n/g, "\n");
    if (/\x1b\[[0-9;]*m/.test(raw)) {
      return { mode: "ansi" as const, raw };
    }
    return { mode: "lines" as const, raw };
  }, [text]);

  if (body.mode === "ansi") {
    const segments = parseAnsiSegments(body.raw);
    return (
      <pre className={`agent-console m-0 whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed ${className}`}>
        {segments.map((seg, i) => (
          <span key={i} className={seg.className}>
            {seg.text}
          </span>
        ))}
      </pre>
    );
  }

  const lines = body.raw.split("\n");
  return (
    <pre className={`agent-console m-0 whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed ${className}`}>
      {lines.map((line, i) => (
        <span key={i} className={logLineClass(line)}>
          {line}
          {i < lines.length - 1 ? "\n" : ""}
        </span>
      ))}
    </pre>
  );
}
