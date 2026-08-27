/**
 * Telling "the link is gone" apart from "the server said no".
 *
 * An SFTP panel holds one session id for its whole life. When the connection drops — the
 * VPS reboots, the network blips, the server times the channel out — that id keeps being
 * used and every call after it fails identically, forever. Reconnecting fixes it; the
 * panel just has to know that it should.
 *
 * The distinction matters in both directions. Reconnecting on a genuine "permission
 * denied" would throw away a working session and re-run the whole handshake to get the
 * same refusal. Not reconnecting on a dead channel leaves a panel that can only be fixed
 * by closing and reopening it, which is what people actually did.
 */
/** Phrases that mean the session or its transport is gone, not that the request was refused. */
const DEAD_SESSION_SIGNS = [
    // The backend no longer has this id: the app re-locked, or it was disconnected.
    "session not found",
    // russh / russh-sftp transport failures.
    "channel closed",
    "channel is closed",
    "connection reset",
    "connection closed",
    "connection aborted",
    "not connected",
    "broken pipe",
    "unexpected eof",
    "early eof",
    "eof while",
    "disconnected",
    "timed out",
    "timeout",
];
/**
 * Should this failure be retried on a fresh session?
 *
 * Deliberately a list of transport phrases rather than "anything that is not a known
 * filesystem error": a new error string we have never seen should default to *showing the
 * user the error*, not to silently tearing down their session and reconnecting.
 */
export function looksLikeDeadSession(message) {
    if (!message)
        return false;
    const m = message.toLowerCase();
    // A refusal is an answer, and an answer proves the link is alive — even when one of the
    // phrases above appears in a filename.
    if (m.includes("permission denied") ||
        m.includes("no such file") ||
        m.includes("not a directory") ||
        m.includes("file exists") ||
        m.includes("directory not empty")) {
        return false;
    }
    return DEAD_SESSION_SIGNS.some((sign) => m.includes(sign));
}
//# sourceMappingURL=sessionHealth.js.map