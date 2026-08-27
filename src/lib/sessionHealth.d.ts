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
/**
 * Should this failure be retried on a fresh session?
 *
 * Deliberately a list of transport phrases rather than "anything that is not a known
 * filesystem error": a new error string we have never seen should default to *showing the
 * user the error*, not to silently tearing down their session and reconnecting.
 */
export declare function looksLikeDeadSession(message: string | null | undefined): boolean;
//# sourceMappingURL=sessionHealth.d.ts.map