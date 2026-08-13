import { describe, expect, it } from "vitest";
import { chatApprovesPlan, classifyChat, looksLikePlan } from "./consent";

describe("classifyChat", () => {
  it("treats the reported phrase as plan approval", () => {
    expect(classifyChat("ok the plan looks good")).toEqual({ kind: "approve" });
    expect(classifyChat("ok looks good")).toEqual({ kind: "approve" });
    expect(classifyChat("  OK the plan looks good.  ")).toEqual({ kind: "approve" });
    expect(classifyChat("lgtm")).toEqual({ kind: "approve" });
    expect(classifyChat("go ahead")).toEqual({ kind: "approve" });
    expect(classifyChat("yes")).toEqual({ kind: "approve" });
    expect(classifyChat("apply and run")).toEqual({ kind: "approve" });
  });

  it("does not treat a new request that starts with ok as approval", () => {
    expect(classifyChat("ok make an plan for the decoy")).toEqual({ kind: "other" });
    expect(classifyChat("ok create a firewall for those bruteforce attacks")).toEqual({
      kind: "other",
    });
  });

  it("classifies reject / cancel / continue", () => {
    expect(classifyChat("no").kind).toBe("reject");
    expect(classifyChat("change the ssh port first").kind).toBe("reject");
    expect(classifyChat("cancel")).toEqual({ kind: "cancel" });
    expect(classifyChat("continue")).toEqual({ kind: "continue" });
  });
});

describe("chatApprovesPlan", () => {
  const plan =
    "## Plan: honeypot on port 22\n\n1. Install Cowrie in a venv on each host\n2. Wire fail2ban to ban on first touch\n3. Verify real SSH on 2222 still works\n";

  it("approves the chat-text plan and continue-after-plan", () => {
    expect(chatApprovesPlan("ok the plan looks good", plan)).toBe(true);
    expect(chatApprovesPlan("continue", plan)).toBe(true);
    expect(chatApprovesPlan("continue", "Sure, I can take a look.")).toBe(false);
    expect(chatApprovesPlan("ok make an plan for the decoy", plan)).toBe(false);
  });
});

describe("looksLikePlan", () => {
  it("detects a numbered plan written in chat", () => {
    const plan = `## Plan: SSH honeypot + auto-ban on port 22 (both hosts)

**Design:** Any connection to port 22 is an attacker.

1. **Install Cowrie** in a Python venv on each host.
2. **Configure it** to listen on 0.0.0.0:22.
3. **systemd service** cowrie.service.
4. **fail2ban jail** maxretry = 1.
5. **Verify** end-to-end.
6. **Confirm** real SSH on 2222 is untouched.

Approve and I'll build it on both hosts.`;
    expect(looksLikePlan(plan)).toBe(true);
    expect(looksLikePlan("I will write a plan next.")).toBe(false);
    expect(looksLikePlan("ok")).toBe(false);
  });
});
