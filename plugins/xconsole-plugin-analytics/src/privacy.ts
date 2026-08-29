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
 * Hook that returns a masking function for hostnames and IPs when privacy mode is enabled.
 */
export function useMaskHost(): (hostOrText: string) => string {
  const isMaskEnabled =
    typeof localStorage !== "undefined"
      ? localStorage.getItem("xconsole-mask-ips") === "1"
      : false;

  return (hostOrText: string) => {
    if (!isMaskEnabled || !hostOrText) return hostOrText;
    return maskIpString(hostOrText);
  };
}
