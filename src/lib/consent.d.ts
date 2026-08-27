/**
 * Frontend mirror of `src-tauri/src/ai/consent.rs`.
 *
 * Chat typed while a plan / question / command-approval is waiting must
 * resolve that waiter. Keep this list in lockstep with the Rust module.
 */
export type ChatIntent = {
    kind: "approve";
} | {
    kind: "reject";
    feedback: string;
} | {
    kind: "cancel";
} | {
    kind: "continue";
} | {
    kind: "other";
};
export declare function classifyChat(text: string): ChatIntent;
export declare function chatApprovesPlan(text: string, previousAssistant: string): boolean;
export declare function looksLikePlan(text: string): boolean;
//# sourceMappingURL=consent.d.ts.map