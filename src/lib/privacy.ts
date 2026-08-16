import { useMemo } from "react";
import { usePrivacyStore } from "../stores/privacyStore";
import { useVpsStore } from "../stores/vpsStore";
import { useCanvasStore } from "../stores/canvasStore";

/**
 * Formats an IP address or hostname into a masked string (e.g. 212.***.***.118).
 * Preserves the first and last octets of IPv4 addresses.
 */
export function maskIpString(ip: string): string {
  if (!ip) return ip;
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
 * Replaces any occurrences of known machine hosts inside an arbitrary string with masked versions.
 */
export function maskMachineIpsInText(
  text: string,
  machineHosts: string[],
  maskEnabled: boolean,
): string {
  if (!maskEnabled || !text) return text;
  let result = text;
  for (const host of machineHosts) {
    if (!host || host.length < 3) continue;
    const masked = maskIpString(host);
    if (masked !== host) {
      result = result.split(host).join(masked);
    }
  }
  return result;
}

/**
 * Hook to retrieve all unique machine hosts from VPS store and open Canvas nodes.
 */
export function useMachineHosts(): string[] {
  const vpsList = useVpsStore((s) => s.vpsList);
  const nodes = useCanvasStore((s) => s.nodes);

  return useMemo(() => {
    const set = new Set<string>();
    for (const v of vpsList) {
      if (v.host && typeof v.host === "string" && v.host.trim()) {
        set.add(v.host.trim());
      }
    }
    for (const n of nodes) {
      const h = (n.data as { host?: string } | undefined)?.host;
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
export function useMaskHost(): (hostOrText: string) => string {
  const maskIps = usePrivacyStore((s) => s.maskIps);
  const machineHosts = useMachineHosts();

  return useMemo(() => {
    return (hostOrText: string) => {
      if (!maskIps || !hostOrText) return hostOrText;
      const trimmed = hostOrText.trim();
      if (machineHosts.includes(trimmed)) {
        return maskIpString(hostOrText);
      }
      return maskMachineIpsInText(hostOrText, machineHosts, true);
    };
  }, [maskIps, machineHosts]);
}
