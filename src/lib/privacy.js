import { useMemo } from "react";
import { usePrivacyStore } from "../stores/privacyStore";
import { useVpsStore } from "../stores/vpsStore";
import { useCanvasStore } from "../stores/canvasStore";
/**
 * Formats an IP address or hostname into a masked string (e.g. 212.***.***.118).
 * Preserves the first and last octets of IPv4 addresses.
 */
export function maskIpString(ip) {
    if (!ip)
        return ip;
    const trimmed = ip.trim();
    // IPv4 pattern: 1.2.3.4 or 1.2.3.4:22
    const ipv4Match = trimmed.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(:\d+)?$/);
    if (ipv4Match) {
        const [, first, , , last, port] = ipv4Match;
        return `${first}.***.***.${last}${port ?? ""}`;
    }
    // IPv6 pattern
    const ipv6Match = trimmed.match(/^([0-9a-fA-F]{1,4}):.+:([0-9a-fA-F]{1,4})(:\d+)?$/);
    if (ipv6Match) {
        const [, first, last, port] = ipv6Match;
        return `${first}:***:***:${last}${port ?? ""}`;
    }
    // Domain / generic hostname masking (e.g. server1.cloud.com -> server1.***.com)
    if (trimmed.includes(".")) {
        const parts = trimmed.split(".");
        if (parts.length >= 2) {
            return `${parts[0]}.***.${parts[parts.length - 1]}`;
        }
    }
    return trimmed.length > 3 ? `${trimmed.slice(0, 2)}***` : "***";
}
/**
 * Masks all IPv4 addresses in an arbitrary text string (e.g. MOTD banners, command outputs, interface IPs).
 * Avoids common non-sensitive addresses like 127.0.0.1 and 0.0.0.0.
 */
export function maskAllIpsInText(text) {
    if (!text)
        return text;
    return text.replace(/\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(:\d+)?\b/g, (match, p1, p2, p3, p4, port) => {
        const n1 = parseInt(p1, 10);
        const n2 = parseInt(p2, 10);
        const n3 = parseInt(p3, 10);
        const n4 = parseInt(p4, 10);
        if (n1 <= 255 && n2 <= 255 && n3 <= 255 && n4 <= 255) {
            if (match === "127.0.0.1" || match === "0.0.0.0")
                return match;
            return `${p1}.***.***.${p4}${port ?? ""}`;
        }
        return match;
    });
}
/**
 * Replaces any occurrences of known machine hosts inside an arbitrary string with masked versions,
 * and masks all detected IPv4 addresses when privacy mode is enabled.
 */
export function maskMachineIpsInText(text, machineHosts, maskEnabled) {
    if (!maskEnabled || !text)
        return text;
    let result = text;
    for (const host of machineHosts) {
        if (!host || host.length < 3)
            continue;
        const masked = maskIpString(host);
        if (masked !== host) {
            result = result.split(host).join(masked);
        }
    }
    result = maskAllIpsInText(result);
    return result;
}
/**
 * Directly returns all known VPS and Canvas node hosts.
 */
export function getKnownHosts() {
    const vpsList = useVpsStore.getState().vpsList;
    const nodes = useCanvasStore.getState().nodes;
    const set = new Set();
    for (const v of vpsList) {
        if (v.host && typeof v.host === "string" && v.host.trim()) {
            set.add(v.host.trim());
        }
    }
    for (const n of nodes) {
        const h = n.data?.host;
        if (h && typeof h === "string" && h.trim()) {
            set.add(h.trim());
        }
    }
    return Array.from(set);
}
/**
 * Masks raw terminal output (Uint8Array or string) for xterm / PTY streaming when privacy mode is on.
 */
export function maskTerminalData(data, machineHosts, maskEnabled) {
    const isEnabled = maskEnabled ?? usePrivacyStore.getState().maskIps;
    const text = typeof data === "string" ? data : new TextDecoder().decode(data);
    if (!isEnabled)
        return text;
    const hosts = machineHosts ?? getKnownHosts();
    return maskMachineIpsInText(text, hosts, true);
}
/**
 * Hook to retrieve all unique machine hosts from VPS store and open Canvas nodes.
 */
export function useMachineHosts() {
    const vpsList = useVpsStore((s) => s.vpsList);
    const nodes = useCanvasStore((s) => s.nodes);
    return useMemo(() => {
        const set = new Set();
        for (const v of vpsList) {
            if (v.host && typeof v.host === "string" && v.host.trim()) {
                set.add(v.host.trim());
            }
        }
        for (const n of nodes) {
            const h = n.data?.host;
            if (h && typeof h === "string" && h.trim()) {
                set.add(h.trim());
            }
        }
        return Array.from(set);
    }, [vpsList, nodes]);
}
/**
 * Hook that returns a masking function `maskHost(str)`:
 * - If privacy mode is ON, replaces machine hostnames/IPs with their masked forms.
 * - If privacy mode is OFF, returns text unchanged.
 */
export function useMaskHost() {
    const maskIps = usePrivacyStore((s) => s.maskIps);
    const machineHosts = useMachineHosts();
    return useMemo(() => {
        return (hostOrText) => {
            if (!maskIps || !hostOrText)
                return hostOrText;
            const trimmed = hostOrText.trim();
            if (machineHosts.includes(trimmed)) {
                return maskIpString(hostOrText);
            }
            return maskMachineIpsInText(hostOrText, machineHosts, true);
        };
    }, [maskIps, machineHosts]);
}
//# sourceMappingURL=privacy.js.map