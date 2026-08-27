/**
 * Formats an IP address or hostname into a masked string (e.g. 212.***.***.118).
 * Preserves the first and last octets of IPv4 addresses.
 */
export declare function maskIpString(ip: string): string;
/**
 * Masks all IPv4 addresses in an arbitrary text string (e.g. MOTD banners, command outputs, interface IPs).
 * Avoids common non-sensitive addresses like 127.0.0.1 and 0.0.0.0.
 */
export declare function maskAllIpsInText(text: string): string;
/**
 * Replaces any occurrences of known machine hosts inside an arbitrary string with masked versions,
 * and masks all detected IPv4 addresses when privacy mode is enabled.
 */
export declare function maskMachineIpsInText(text: string, machineHosts: string[], maskEnabled: boolean): string;
/**
 * Directly returns all known VPS and Canvas node hosts.
 */
export declare function getKnownHosts(): string[];
/**
 * Masks raw terminal output (Uint8Array or string) for xterm / PTY streaming when privacy mode is on.
 */
export declare function maskTerminalData(data: Uint8Array | string, machineHosts?: string[], maskEnabled?: boolean): string;
/**
 * Hook to retrieve all unique machine hosts from VPS store and open Canvas nodes.
 */
export declare function useMachineHosts(): string[];
/**
 * Hook that returns a masking function `maskHost(str)`:
 * - If privacy mode is ON, replaces machine hostnames/IPs with their masked forms.
 * - If privacy mode is OFF, returns text unchanged.
 */
export declare function useMaskHost(): (hostOrText: string) => string;
//# sourceMappingURL=privacy.d.ts.map