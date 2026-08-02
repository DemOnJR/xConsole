import { describe, expect, it } from "vitest";
import { looksLikeDeadSession } from "./sessionHealth";

describe("looksLikeDeadSession", () => {
  /// The failures that leave an SFTP panel permanently broken until it is closed.
  it("recognises a link that has gone away", () => {
    for (const msg of [
      "SFTP session not found",
      "list failed: channel closed",
      "list failed: Connection reset by peer",
      "list failed: unexpected EOF",
      "list failed: Broken pipe",
      "list failed: operation timed out",
      "Disconnected",
    ]) {
      expect(looksLikeDeadSession(msg), msg).toBe(true);
    }
  });

  /// A refusal is an answer, and an answer proves the link is alive. Reconnecting here
  /// would tear down a working session and re-run the handshake for the same refusal.
  it("does not reconnect when the server simply said no", () => {
    for (const msg of [
      "list failed: Permission denied",
      "list failed: No such file or directory",
      "rename failed: File exists",
      "rmdir failed: Directory not empty",
      "list failed: Not a directory",
    ]) {
      expect(looksLikeDeadSession(msg), msg).toBe(false);
    }
  });

  /// A filename can contain anything, including our own signal words. The refusal check
  /// runs first precisely so a file called "timeout" cannot trigger a reconnect.
  it("is not fooled by a filename that contains a signal word", () => {
    expect(looksLikeDeadSession("list failed: No such file or directory: /var/timeout")).toBe(
      false,
    );
    expect(
      looksLikeDeadSession("list failed: Permission denied: /srv/disconnected"),
    ).toBe(false);
  });

  /// An unrecognised failure shows the user the error rather than silently reconnecting.
  it("defaults to leaving the session alone", () => {
    expect(looksLikeDeadSession("some error nobody has seen before")).toBe(false);
    expect(looksLikeDeadSession("")).toBe(false);
    expect(looksLikeDeadSession(null)).toBe(false);
    expect(looksLikeDeadSession(undefined)).toBe(false);
  });
});
