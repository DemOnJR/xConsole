/**
 * Reading what the WhatsApp helper says about itself.
 *
 * The helper is a separate Go binary, and rebuilding the app does not touch it — so an
 * otherwise current install can be driving one that predates half its features. That
 * shows up as a feature which looks broken rather than out of date.
 */

/**
 * Whether an error means the helper binary is behind the app.
 *
 * It says so in two different voices, from two different places. The host says the
 * helper "did not answer" when a query goes unanswered; the helper itself says it "does
 * not understand" a command it is too old to know. Both mean the same fix, and matching
 * only one is how the rebuild button ended up missing from the place the user actually
 * saw the error.
 */
export function isHelperTooOld(message: string): boolean {
  return /older than the xConsole|does not understand|did not answer/i.test(message);
}
