export type AgentRuntimeMode = "auto" | "standard" | "code" | "plan" | "minimal";
/**
 * Classifies prompt intent to dynamically resolve mode when in "auto" mode.
 * - "plan": planning, designing, architecting, step-by-step roadmap, analyzing before mutations.
 * - "code": writing code, implementing features, fixing bugs, refactoring files.
 * - "minimal": short casual greetings, brief thank-yous, tiny status inquiries.
 * - "standard": general questions, multi-faceted tasks, DevOps operations.
 */
export declare function resolveEffectiveMode(mode: AgentRuntimeMode, prompt: string): "standard" | "code" | "plan" | "minimal";
//# sourceMappingURL=agentMode.d.ts.map