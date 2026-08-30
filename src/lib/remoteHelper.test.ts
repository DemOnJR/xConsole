import { describe, expect, it } from "vitest";
import { isHelperTooOld } from "./remoteHelper";

describe("isHelperTooOld", () => {
  it("recognises both voices the same problem speaks in", () => {
    // From the host, when a query goes unanswered.
    expect(
      isHelperTooOld(
        "the WhatsApp helper did not answer. It is probably an older build that does not know how to list chats",
      ),
    ).toBe(true);
    // From the helper itself, rejecting a command it does not know. This is the one the
    // user hit, and the one the button was missing from.
    expect(
      isHelperTooOld(
        'this WhatsApp helper does not understand "presence" — it is older than the xConsole running it.',
      ),
    ).toBe(true);
  });

  it("does not offer a rebuild for problems a rebuild cannot fix", () => {
    // Sending the user to rebuild a binary when the real problem is their phone or
    // their network wastes their time and hides the actual cause.
    expect(isHelperTooOld("WhatsApp unlinked this device — scan again to reconnect")).toBe(false);
    expect(isHelperTooOld("the pairing code expired — start again")).toBe(false);
    expect(isHelperTooOld("could not connect to WhatsApp: dial tcp: i/o timeout")).toBe(false);
    expect(isHelperTooOld("")).toBe(false);
  });
});
