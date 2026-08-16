import { describe, expect, it } from "vitest";
import { effectiveMode, isAllowlisted, isReadOnly, shouldAutoRun } from "./safety";

describe("isReadOnly", () => {
  it("accepts read-only commands", () => {
    expect(isReadOnly("ls -la")).toBe(true);
    expect(isReadOnly("cat /etc/os-release")).toBe(true);
    expect(isReadOnly("git status")).toBe(true);
    expect(isReadOnly("df -h")).toBe(true);
  });

  it("rejects writes, redirects and substitution", () => {
    expect(isReadOnly("rm -rf /")).toBe(false);
    expect(isReadOnly("echo hi > /etc/passwd")).toBe(false);
    expect(isReadOnly("echo $(rm -rf /)")).toBe(false);
    expect(isReadOnly("cat `whoami`")).toBe(false);
  });

  it("splits on shell separators AND newlines (prompt-injection guard)", () => {
    expect(isReadOnly("ls; rm -rf /")).toBe(false);
    expect(isReadOnly("ls | grep x")).toBe(true);
    expect(isReadOnly("ls\nrm -rf /")).toBe(false);
    expect(isReadOnly("ls\r\nrm -rf /")).toBe(false);
    expect(isReadOnly("cat f && reboot")).toBe(false);
  });

  it("handles env-assignment prefixes and sudo word", () => {
    expect(isReadOnly("FOO=bar ls")).toBe(true);
    expect(isReadOnly("sudo ls")).toBe(true);
    expect(isReadOnly("sudo rm -rf /")).toBe(false);
  });

  it("rejects find with mutating predicates", () => {
    expect(isReadOnly("find / -name x")).toBe(true);
    expect(isReadOnly("find / -delete")).toBe(false);
    expect(isReadOnly("find / -exec rm {} \\;")).toBe(false);
  });
});

describe("isAllowlisted", () => {
  it("auto-runs read-only commands", () => {
    expect(isAllowlisted("ls -la")).toBe(true);
    expect(isAllowlisted("cat /etc/os-release")).toBe(true);
    expect(isAllowlisted("git status")).toBe(true);
    expect(isAllowlisted("df -h")).toBe(true);
  });

  it("blocks sensitive-path reads (case-insensitive)", () => {
    expect(isAllowlisted("cat ~/.ssh/id_rsa")).toBe(false);
    expect(isAllowlisted("cat .env")).toBe(false);
    expect(isAllowlisted("grep -r secret /home/u/.aws")).toBe(false);
    expect(isAllowlisted("cat /etc/shadow")).toBe(false);
    expect(isAllowlisted("cat ~/.ssh/id_ed25519")).toBe(false);
  });

  it("blocks mutating commands and separator chains", () => {
    expect(isAllowlisted("rm -rf /tmp/x")).toBe(false);
    expect(isAllowlisted("apt install nginx")).toBe(false);
    expect(isAllowlisted("git push")).toBe(false);
    expect(isAllowlisted("ls; rm -rf /")).toBe(false);
    expect(isAllowlisted("ls\nrm -rf /")).toBe(false);
  });
});

describe("effectiveMode", () => {
  it("prefers per-vps override over global", () => {
    expect(effectiveMode("approve", "v1", { v1: "full" })).toBe("full");
    expect(effectiveMode("full", "v1", { v1: "approve" })).toBe("approve");
  });

  it("falls back to global then approve", () => {
    expect(effectiveMode("allowlist", undefined, {})).toBe("allowlist");
    expect(effectiveMode(undefined, "v1", {})).toBe("approve");
    expect(effectiveMode("bogus", undefined, {})).toBe("approve");
  });
});

describe("shouldAutoRun", () => {
  it("full always runs", () => {
    expect(shouldAutoRun("full", "rm -rf /")).toBe(true);
  });

  it("allowlist runs only read-only", () => {
    expect(shouldAutoRun("allowlist", "ls")).toBe(true);
    expect(shouldAutoRun("allowlist", "apt install nginx")).toBe(false);
  });

  it("approve never auto-runs", () => {
    expect(shouldAutoRun("approve", "ls")).toBe(false);
  });
});
