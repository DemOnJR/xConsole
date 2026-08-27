/**
 * Classifies prompt intent to dynamically resolve mode when in "auto" mode.
 * - "plan": planning, designing, architecting, step-by-step roadmap, analyzing before mutations.
 * - "code": writing code, implementing features, fixing bugs, refactoring files.
 * - "minimal": short casual greetings, brief thank-yous, tiny status inquiries.
 * - "standard": general questions, multi-faceted tasks, DevOps operations.
 */
export function resolveEffectiveMode(mode, prompt) {
    if (mode !== "auto")
        return mode;
    const text = prompt.trim().toLowerCase();
    if (!text)
        return "standard";
    // 1. Planning intent detection (English & Romanian)
    const planKeywords = [
        /\bplan\b/,
        /\bplanning\b/,
        /\bplanifica\b/,
        /\bplanificare\b/,
        /\bcum facem\b/,
        /\bcum sa facem\b/,
        /\bpropose (a )?plan\b/,
        /\bmake a plan\b/,
        /\bcreate a plan\b/,
        /\bstep[- ]by[- ]step plan\b/,
        /\barchitect\b/,
        /\barchitecture\b/,
        /\broadmap\b/,
        /\bstrategy\b/,
        /\banalyze before\b/,
        /\blook into before changing\b/,
        /\bdo not change anything yet\b/,
        /\bfa un plan\b/,
    ];
    if (planKeywords.some((pattern) => pattern.test(text))) {
        return "plan";
    }
    // 2. Direct coding intent detection
    const codeKeywords = [
        /\bwrite code\b/,
        /\bimplement\b/,
        /\bcreate (a |an )?(component|function|file|script|class|module|endpoint|api)\b/,
        /\bfix (the |a )?(bug|error|issue|crash|typo|exception)\b/,
        /\brefactor\b/,
        /\bscrie cod\b/,
        /\bimplementeaza\b/,
        /\brepara\b/,
        /\badd test\b/,
        /\bunit test\b/,
        /\btypescript\b/,
        /\brust\b/,
        /\bpython\b/,
        /\brewrite\b/,
    ];
    if (codeKeywords.some((pattern) => pattern.test(text))) {
        return "code";
    }
    // 3. Short casual / minimal intent detection
    if (text.length <= 20) {
        const stripped = text.replace(/^[!.,?]+|[!.,?]+$/g, "").trim();
        const casualWords = [
            "hi",
            "hello",
            "hey",
            "salut",
            "buna",
            "hei",
            "thanks",
            "thank you",
            "multumesc",
            "ms",
            "mersi",
            "ty",
            "ok",
            "lgtm",
        ];
        if (casualWords.includes(stripped) || casualWords.some((w) => stripped.startsWith(w + " "))) {
            return "minimal";
        }
    }
    return "standard";
}
//# sourceMappingURL=agentMode.js.map