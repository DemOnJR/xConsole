/**
 * Frontend mirror of the backend's safety logic (`src-tauri/src/ai/safety.rs`) so the
 * Execute button can decide auto-run vs type-and-wait without a round-trip.
 */

export type SafetyMode = "full" | "allowlist" | "approve";

/** Read-only commands that may auto-run under allowlist mode (backend mirror). */
const READ_ONLY_PREFIXES = [
  "ls",
  "cat",
  "head",
  "tail",
  "grep",
  "find",
  "pwd",
  "whoami",
  "uname",
  "df",
  "du",
  "free",
  "ps",
  "top",
  "uptime",
  "which",
  "type",
  "echo",
  "git status",
  "git log",
  "git branch",
  "git diff",
  "git remote",
  "systemctl status",
  "journalctl",
];

const SENSITIVE_MARKERS = [".ssh", ".aws", ".env", "environ", "id_rsa", "credentials", "password=", "--token ", "x-api-key"];

/** Whether a command may auto-run under allowlist safety mode. */
export function isAllowlisted(command: string): boolean {
  const cmd = command.trim();
  if (cmd.startsWith("#") || cmd.startsWith("//")) return true; // comment
  const hasDanger = SENSITIVE_MARKERS.some((m) => cmd.includes(m));
  if (hasDanger) return false;
  return READ_ONLY_PREFIXES.some((p) => cmd.startsWith(p));
}

/** The effective safety mode for a vps (global setting + per-vps override). */
export function effectiveMode(
  globalMode: string | undefined,
  vpsId: string | undefined,
  perVps: Record<string, string>,
): SafetyMode {
  const v = vpsId ? perVps[vpsId] : undefined;
  const mode = v || globalMode || "approve";
  return mode === "full" || mode === "allowlist" ? mode : "approve";
}

/** Should Execute auto-run the command (vs type-and-wait)? */
export function shouldAutoRun(mode: SafetyMode, command: string): boolean {
  if (mode === "full") return true;
  if (mode === "allowlist") return isAllowlisted(command);
  return false;
}
