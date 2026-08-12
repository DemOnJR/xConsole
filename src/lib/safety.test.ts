import { describe, expect, it } from "vitest";
import { effectiveMode, isAllowlisted, shouldAutoRun } from "./safety";

describe("isAllowlisted", () => {
  it("auto-runs read-only commands", () => {
    expect(isAllowlisted("ls -la")).toBe(true);
    expect(isAllowlisted("cat /etc/os-release")).toBe(true);
    expect(isAllowlisted("git status")).toBe(true);
    expect(isAllowlisted("df -h")).toBe(true);
  });

  it("blocks sensitive-path reads", () => {
    expect(isAllowlisted("cat ~/.ssh/id_rsa")).toBe(false);
    expect(isAllowlisted("cat .env")).toBe(false);
    expect(isAllowlisted("grep -r secret /home/u/.aws")).toBe(false);
  });

  it("blocks mutating commands", () => {
    expect(isAllowlisted("rm -rf /tmp/x")).toBe(false);
    expect(isAllowlisted("apt install nginx")).toBe(false);
    expect(isAllowlisted("git push")).toBe(false);
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
