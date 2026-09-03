import { describe, expect, it } from "vitest";
import { chatCanApproveCommand, chatRejectsCommand } from "./consent";
import { parseApproval, type AgentApproval } from "./tauri";

const approval = (command: string): AgentApproval => ({
  id: "a1",
  session_id: "s1",
  command,
  status: "pending",
});

/** What `safety::authorize` actually concatenates onto the command it asks about. */
const destructive = approval(
  "rm -rf /srv/app\n\n" +
    "THIS CANNOT BE UNDONE — it would delete /srv/app.\n" +
    "Deleted files are not recoverable from this host.\n\n" +
    "What is there right now:\n" +
    "drwxr-xr-x 12 root root 4096 2026-09-01 app\n" +
    "41203 files, 12G",
);

describe("parseApproval", () => {
  it("separates the command from the blast radius the backend measured", () => {
    // The backend appends all of this to `command`, so the UI had one string in which
    // `rm -rf /srv/app` and `ls -la` looked alike inside a small scrolling box.
    const parsed = parseApproval(destructive);
    expect(parsed.command).toBe("rm -rf /srv/app");
    expect(parsed.irreversible).toBe(true);
    expect(parsed.why).toContain("delete /srv/app");
    expect(parsed.preview).toContain("41203 files");
  });

  it("leaves an ordinary command exactly as it is", () => {
    const parsed = parseApproval(approval("systemctl restart nginx"));
    expect(parsed.command).toBe("systemctl restart nginx");
    expect(parsed.irreversible).toBe(false);
    expect(parsed.why).toBe("");
    expect(parsed.preview).toBe("");
  });
});

describe("approving from the chat box", () => {
  it("accepts a casual yes for an ordinary command", () => {
    // Making someone reach for the mouse to confirm a service restart trains them to
    // stop reading the cards at all.
    for (const yes of ["ok", "k", "sure", "go", "yes", "do it"]) {
      expect(chatCanApproveCommand(yes, false)).toBe(true);
    }
  });

  it("refuses the same words when the command cannot be undone", () => {
    // The vocabulary is identical to approving a plan or agreeing with a sentence, so a
    // "k" typed into chat is not evidence that anyone read the path.
    for (const yes of ["ok", "k", "sure", "go", "yes", "do it"]) {
      expect(chatCanApproveCommand(yes, true)).toBe(false);
    }
  });

  it("always honours a refusal, however dangerous the command", () => {
    // Refusing to act on "no" because the command was destructive would be backwards.
    for (const no of ["no", "stop", "cancel", "don't"]) {
      expect(chatRejectsCommand(no)).toBe(true);
    }
  });

  it("does not treat a new instruction as consent", () => {
    expect(chatCanApproveCommand("check the nginx config first", false)).toBe(false);
  });
});
