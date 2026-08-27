/**
 * Frontend mirror of the backend's safety logic (`src-tauri/src/ai/safety.rs`) so the
 * Execute button can decide auto-run vs type-and-wait without a round-trip.
 * Keep in sync with the Rust implementation.
 */
export type SafetyMode = "full" | "allowlist" | "approve";
/** Whether a whole command line is read-only (backend mirror incl. newline split). */
export declare function isReadOnly(command: string): boolean;
/** Whether a command may auto-run under allowlist safety mode (backend mirror). */
export declare function isAllowlisted(command: string): boolean;
/** The effective safety mode for a vps (global setting + per-vps override). */
export declare function effectiveMode(globalMode: string | undefined, vpsId: string | undefined, perVps: Record<string, string>): SafetyMode;
/** Should Execute auto-run the command (vs type-and-wait)? */
export declare function shouldAutoRun(mode: SafetyMode, command: string): boolean;
//# sourceMappingURL=safety.d.ts.map