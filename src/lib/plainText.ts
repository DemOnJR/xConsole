/**
 * Markdown → plain text, for terminal-style rendering. The agent window is a REPL,
 * so assistant/user content renders as wrapped monospace text instead of styled
 * markdown bubbles. This strips formatting without touching the raw content.
 */

const CODE_FENCE = /```(\w*)\n?([\s\S]*?)```/g;

/** Strip markdown formatting to readable plain text. */
export function plainText(md: string): string {
  if (!md) return "";
  let out = md;
  // Code fences → keep the code, drop the fence markers + language.
  out = out.replace(CODE_FENCE, (_m, _lang, code) => code);
  // Inline code → backticks removed.
  out = out.replace(/`([^`]+)`/g, "$1");
  // Images → alt text; links → text (drop URLs).
  out = out.replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1");
  out = out.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");
  // Bold / italic / strikethrough markers.
  out = out.replace(/\*\*([^*]+)\*\*/g, "$1");
  out = out.replace(/\*([^*]+)\*/g, "$1");
  out = out.replace(/__([^_]+)__/g, "$1");
  out = out.replace(/_([^_]+)_/g, "$1");
  out = out.replace(/~~([^~]+)~~/g, "$1");
  // Headings → plain lines.
  out = out.replace(/^#{1,6}\s+/gm, "");
  // Blockquote markers.
  out = out.replace(/^>\s?/gm, "");
  // List markers → keep as dashes for readability.
  out = out.replace(/^\s*[-*+]\s+/gm, "- ");
  out = out.replace(/^\s*\d+\.\s+/gm, "- ");
  // Horizontal rules.
  out = out.replace(/^\s*(---|\*\*\*|___)\s*$/gm, "");
  // HTML tags (best effort — no script/style content).
  out = out.replace(/<[^>]+>/g, "");
  // Collapse 3+ newlines to 2.
  out = out.replace(/\n{3,}/g, "\n\n");
  return out.trim();
}
