/**
 * Frontend mirror of the backend's safety logic (`src-tauri/src/ai/safety.rs`) so the
 * Execute button can decide auto-run vs type-and-wait without a round-trip.
 * Keep in sync with the Rust implementation.
 */

export type SafetyMode = "full" | "allowlist" | "approve";

/** Commands whose leading token is considered read-only / safe (backend mirror). */
const READ_ONLY = new Set([
  "ls", "cat", "pwd", "whoami", "id", "date", "uptime", "df", "du", "free", "ps", "top", "htop",
  "stat", "head", "tail", "wc", "grep", "egrep", "rg", "find", "echo", "printf", "uname",
  "hostname", "which", "type", "ip", "ss", "netstat", "ping", "dig", "nslookup",
  "tree", "file", "readlink", "realpath", "history", "lsblk", "lscpu", "lsof", "dmesg",
  "journalctl", "true", "test",
]);

/** `find` predicates that execute commands or mutate the filesystem (backend mirror). */
const FIND_WRITE_PREDICATES = new Set([
  "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprintf", "-fls",
]);

/** Substrings naming credential/secret stores (backend mirror, lowercased match). */
const SENSITIVE_PATH_MARKERS = [
  "/.ssh", "\\.ssh", "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa", "authorized_keys",
  "/.aws", "\\.aws", "/.gnupg", "\\.gnupg", "gcloud", "/.kube", "\\.kube", "/.docker/config",
  ".env", "credential", "secret", "private_key", "privatekey", "passwd", "shadow",
  ".pem", ".pfx", ".p12", ".key", ".keystore", ".netrc", ".pgpass", "wallet",
  "xconsole.db", "db.lock.json", "id_rsa.pub",
  "environ",
];

/** Shell metacharacters that redirect output/input or run a nested command. */
function hasWriteOrSubstitution(command: string): boolean {
  return command.includes(">") || command.includes("<") || command.includes("`") || command.includes("$(");
}

/** Whether a whole command line is read-only (backend mirror incl. newline split). */
export function isReadOnly(command: string): boolean {
  if (hasWriteOrSubstitution(command)) return false;
  const segments = command.split(/[|;&\n\r]/).filter((s) => s.trim().length > 0);
  let any = false;
  for (const rawSeg of segments) {
    any = true;
    const seg = rawSeg.trim();
    // Skip leading env-var assignments, then drop a leading `sudo` word.
    const tokens = seg.split(/\s+/);
    let i = 0;
    while (i < tokens.length && tokens[i].includes("=")) i++;
    let token = tokens[i] ?? "";
    if (token === "sudo") token = tokens[i + 1] ?? "";
    // `find` is read-only only without mutating/executing predicates.
    if (token === "find" && tokens.some((t) => FIND_WRITE_PREDICATES.has(t))) return false;
    if (!READ_ONLY.has(token)) {
      const lc = seg.toLowerCase();
      const statusOk =
        lc.startsWith("systemctl status") ||
        lc.startsWith("docker ps") ||
        lc.startsWith("git status") ||
        lc.startsWith("git log") ||
        lc.startsWith("git diff") ||
        lc.startsWith("git show");
      if (!statusOk) return false;
    }
  }
  return any;
}

/** Terraform plan/validate/fmt/show are read-only; apply/destroy never auto-run (backend mirror). */
function isTerraformReadonly(command: string): boolean {
  const lc = command.toLowerCase();
  if (
    lc.includes("terraform apply") ||
    lc.includes("terraform destroy") ||
    lc.includes("terraform import") ||
    lc.includes("tfc remote apply") ||
    lc.includes("-replace")
  ) {
    return false;
  }
  return (
    lc.startsWith("tfc remote plan") ||
    lc.includes("terraform plan") ||
    lc.includes("terraform validate") ||
    lc.includes("terraform fmt") ||
    lc.includes("terraform show") ||
    lc.includes("terraform version") ||
    lc.includes("terraform output") ||
    lc.includes("terraform providers") ||
    lc.includes("local terraform plan") ||
    lc.includes("local terraform validate") ||
    lc.includes("local terraform fmt") ||
    lc.includes("local terraform show") ||
    lc.includes("local terraform init")
  );
}

/** Whether a command line references a likely credential/secret path (backend mirror). */
function touchesSensitivePath(command: string): boolean {
  const lc = command.toLowerCase();
  return SENSITIVE_PATH_MARKERS.some((m) => lc.includes(m));
}

/** Whether a command may auto-run under allowlist safety mode (backend mirror). */
export function isAllowlisted(command: string): boolean {
  return (
    (isReadOnly(command) || isTerraformReadonly(command)) && !touchesSensitivePath(command)
  );
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
