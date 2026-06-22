import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { MarkdownCodeBlock } from "./SyntaxHighlight";

/** Strip accidental whole-message code fences some models wrap replies in. */
export function normalizeAgentMarkdown(content: string): string {
  const trimmed = content.trim();
  const fence = trimmed.match(/^```(?:\w+)?\n([\s\S]*)\n```$/);
  if (fence) return fence[1].trimEnd();
  return content;
}

const assistantComponents: Components = {
  h1: ({ children }) => (
    <h1 className="mb-3 mt-1 text-base font-semibold text-gray-50">{children}</h1>
  ),
  h2: ({ children }) => (
    <h2 className="mb-2 mt-4 text-sm font-semibold text-gray-100">{children}</h2>
  ),
  h3: ({ children }) => (
    <h3 className="mb-1.5 mt-3 text-sm font-medium text-gray-200">{children}</h3>
  ),
  p: ({ children }) => <p className="mb-2 last:mb-0 leading-relaxed">{children}</p>,
  ul: ({ children }) => (
    <ul className="mb-2 ml-4 list-disc space-y-1 marker:text-gray-500">{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className="mb-2 ml-4 list-decimal space-y-1 marker:text-gray-500">{children}</ol>
  ),
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  strong: ({ children }) => <strong className="font-semibold text-gray-50">{children}</strong>,
  em: ({ children }) => <em className="text-gray-300 italic">{children}</em>,
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="text-blue-400 underline decoration-blue-400/40 hover:text-blue-300"
    >
      {children}
    </a>
  ),
  blockquote: ({ children }) => (
    <blockquote className="my-2 border-l-2 border-[#334155] pl-3 text-gray-400 italic">
      {children}
    </blockquote>
  ),
  hr: () => <hr className="my-3 border-[#1f2737]" />,
  code: ({ className, children }) => {
    const text = String(children).replace(/\n$/, "");
    const isBlock = className?.includes("language-") || text.includes("\n");
    if (isBlock) {
      return <MarkdownCodeBlock code={text} className={className} />;
    }
    return (
      <code className="rounded bg-[#1a2230] px-1 py-0.5 font-mono text-[12px] text-emerald-200/90">
        {children}
      </code>
    );
  },
  table: ({ children }) => (
    <div className="my-3 overflow-x-auto rounded-md border border-[#1f2737]">
      <table className="w-full min-w-[280px] border-collapse text-left text-[12px]">
        {children}
      </table>
    </div>
  ),
  thead: ({ children }) => <thead className="bg-[#151b26] text-gray-300">{children}</thead>,
  tbody: ({ children }) => <tbody className="divide-y divide-[#1f2737]">{children}</tbody>,
  tr: ({ children }) => <tr className="hover:bg-[#11161f]/80">{children}</tr>,
  th: ({ children }) => (
    <th className="border-b border-[#1f2737] px-2.5 py-1.5 font-medium">{children}</th>
  ),
  td: ({ children }) => <td className="px-2.5 py-1.5 text-gray-300">{children}</td>,
};

export function AgentMarkdown({
  content,
  variant = "assistant",
}: {
  content: string;
  variant?: "assistant" | "user";
}) {
  const body = normalizeAgentMarkdown(content);

  if (variant === "user") {
    return <span className="whitespace-pre-wrap">{body}</span>;
  }

  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={assistantComponents}>
      {body}
    </ReactMarkdown>
  );
}
