/**
 * Frontend mirror of `src-tauri/src/ai/consent.rs`.
 *
 * Chat typed while a plan / question / command-approval is waiting must
 * resolve that waiter. Keep this list in lockstep with the Rust module.
 */
export function classifyChat(text) {
    const raw = text.trim();
    if (!raw)
        return { kind: "other" };
    const t = normalize(raw);
    if (isExact(t, CANCEL))
        return { kind: "cancel" };
    if (isExact(t, REJECT))
        return { kind: "reject", feedback: raw };
    const rejected = stripPrefixList(t, REJECT_PREFIX);
    if (rejected !== null) {
        return { kind: "reject", feedback: rejected || raw };
    }
    if (isExact(t, CONTINUE))
        return { kind: "continue" };
    if (isExact(t, APPROVE))
        return { kind: "approve" };
    if (mentionsPlanApproval(t) && !looksLikeNewTask(t))
        return { kind: "approve" };
    const rest = stripPrefixList(t, APPROVE_PREFIX);
    if (rest !== null) {
        if (!rest)
            return { kind: "approve" };
        if (mentionsPlan(t) || !looksLikeNewTask(rest))
            return { kind: "approve" };
        return { kind: "other" };
    }
    return { kind: "other" };
}
export function chatApprovesPlan(text, previousAssistant) {
    const intent = classifyChat(text);
    if (intent.kind === "approve")
        return true;
    if (intent.kind === "continue")
        return looksLikePlan(previousAssistant);
    return false;
}
export function looksLikePlan(text) {
    const t = text.trim();
    if (t.length < 80)
        return false;
    const steps = countNumberedSteps(t);
    if (steps >= 2)
        return true;
    const lower = t.toLowerCase();
    const heading = lower.split(/\r?\n/).some((line) => {
        const s = line.trim();
        return (s.startsWith("## ") || s.startsWith("# ") || s.startsWith("### ")) && s.includes("plan");
    });
    if (heading && t.length >= 160)
        return true;
    if (steps >= 1 && t.length >= 160 && (lower.includes("approve") || heading))
        return true;
    return false;
}
function normalize(s) {
    return s
        .replace(/[^a-zA-Z0-9'\s-]+/g, "")
        .replace(/-/g, " ")
        .replace(/\s+/g, " ")
        .trim()
        .toLowerCase();
}
function isExact(t, list) {
    return list.includes(t);
}
function stripPrefixList(t, prefixes) {
    let best = null;
    for (const p of prefixes) {
        if (t.startsWith(p)) {
            const rest = t.slice(p.length).trimStart();
            if (best === null || rest.length < best.length)
                best = rest;
        }
    }
    return best;
}
function mentionsPlan(t) {
    return t.includes("plan") || t.includes("steps");
}
function mentionsPlanApproval(t) {
    return mentionsPlan(t) && APPROVE_HINT.some((h) => t.includes(h)) && !looksLikeNewTask(t);
}
function looksLikeNewTask(t) {
    return NEW_TASK.some((v) => t.startsWith(v) || t.includes(` ${v}`) || t.includes(` ${v.trimEnd()}`));
}
function countNumberedSteps(text) {
    let n = 0;
    for (const line of text.split(/\r?\n/)) {
        const s = line.trimStart();
        const m = /^(\d+)[.)]/.exec(s);
        if (m)
            n += 1;
    }
    return n;
}
const APPROVE = [
    "ok",
    "okay",
    "k",
    "yes",
    "y",
    "yep",
    "yeah",
    "yas",
    "sure",
    "ok sure",
    "yes please",
    "yes do it",
    "approve",
    "approved",
    "i approve",
    "lgtm",
    "looks good",
    "looks great",
    "looks fine",
    "sounds good",
    "sounds great",
    "perfect",
    "great",
    "good",
    "go",
    "go ahead",
    "go for it",
    "do it",
    "do it now",
    "just do it",
    "ship it",
    "ship",
    "execute",
    "execute it",
    "run it",
    "run them",
    "apply",
    "apply it",
    "apply and run",
    "proceed",
    "please proceed",
    "lets go",
    "let's go",
    "lets do it",
    "let's do it",
    "ok go",
    "ok do it",
    "ok proceed",
    "ok execute",
    "ok apply",
    "ok run it",
    "ok looks good",
    "okay looks good",
    "ok the plan looks good",
    "okay the plan looks good",
    "the plan looks good",
    "plan looks good",
    "looks good to me",
    "thats fine",
    "that's fine",
    "thats good",
    "that's good",
    "all good",
    "good to go",
];
const APPROVE_PREFIX = [
    "ok the plan looks good",
    "okay the plan looks good",
    "ok looks good",
    "okay looks good",
    "looks good",
    "lgtm",
    "approved",
    "approve",
    "go ahead",
    "sounds good",
    "ok go",
    "okay go",
];
const APPROVE_HINT = [
    "looks good",
    "lgtm",
    "approve",
    "approved",
    "go ahead",
    "do it",
    "ship it",
    "sounds good",
    "good to go",
];
const REJECT = [
    "no",
    "nope",
    "nah",
    "reject",
    "rejected",
    "deny",
    "denied",
    "dont",
    "don't",
    "do not",
    "no thanks",
    "not yet",
    "not now",
    "hold on",
    "wait",
    "stop",
];
const REJECT_PREFIX = [
    "reject",
    "rejected",
    "no ",
    "nope ",
    "dont ",
    "don't ",
    "do not ",
    "change ",
    "revise ",
    "instead ",
];
const CANCEL = ["cancel", "cancelled", "canceled", "nevermind", "never mind", "forget it", "abort"];
const CONTINUE = [
    "continue",
    "keep going",
    "keep on",
    "go on",
    "resume",
    "carry on",
    "dont stop",
    "don't stop",
];
const NEW_TASK = [
    "make ",
    "make a",
    "make an",
    "create ",
    "add ",
    "build ",
    "write ",
    "implement ",
    "fix ",
    "check ",
    "investigate ",
    "plan ",
    "draft ",
];
//# sourceMappingURL=consent.js.map