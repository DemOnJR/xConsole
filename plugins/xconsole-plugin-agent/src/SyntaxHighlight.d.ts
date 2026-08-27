/** Map common fence / alias names to a registered highlight.js language id. */
export declare function normalizeLanguage(raw?: string): string | undefined;
/** Extract language from react-markdown `className` (e.g. `language-python`). */
export declare function langFromMarkdownClass(className?: string): string | undefined;
export declare function langFromPath(path: string): string | undefined;
/** Bash / shell command with syntax colors. */
export declare const ShellCommand: import("react").NamedExoticComponent<{
    code: string;
    className?: string;
}>;
/** Source code block (markdown fences, file diffs, etc.). */
export declare const CodeHighlight: import("react").NamedExoticComponent<{
    code: string;
    language?: string;
    className?: string;
}>;
/** Fenced code block for agent markdown replies. Copy + Execute (for shell blocks
 *  with a resolvable host) live in the header. */
export declare const MarkdownCodeBlock: import("react").NamedExoticComponent<{
    code: string;
    className?: string;
    /** When set and the block is shell-ish, an Execute button is shown. */
    executeTarget?: {
        name: string;
        host: string;
    } | null;
    onExecute?: (code: string) => void;
}>;
/** Terminal / SSH output — ANSI colors + log-level line tinting. */
export declare const ConsoleOutput: import("react").NamedExoticComponent<{
    text: string;
    className?: string;
}>;
//# sourceMappingURL=SyntaxHighlight.d.ts.map
