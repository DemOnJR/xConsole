import { describe, expect, it } from "vitest";
import { maskIpString, maskMachineIpsInText, maskAllIpsInText, maskTerminalData } from "./privacy";

describe("privacy masking", () => {
  it("masks IPv4 addresses keeping first and last octet", () => {
    expect(maskIpString("212.158.42.118")).toBe("212.***.***.118");
    expect(maskIpString("192.168.1.1")).toBe("192.***.***.1");
    expect(maskIpString("10.0.0.1")).toBe("10.***.***.1");
    expect(maskIpString("212.158.42.118:22")).toBe("212.***.***.118:22");
  });

  it("masks IPv6 addresses keeping first and last segment", () => {
    expect(maskIpString("2001:0db8:85a3:0000:0000:8a2e:0370:7334")).toBe("2001:***:***:7334");
  });

  it("masks machine IPs inside arbitrary text only when enabled", () => {
    const text = "Connecting to root@212.158.42.118 on port 22. Status ok on 212.158.42.118.";
    const machineHosts = ["212.158.42.118"];

    const masked = maskMachineIpsInText(text, machineHosts, true);
    expect(masked).toBe("Connecting to root@212.***.***.118 on port 22. Status ok on 212.***.***.118.");

    const unmasked = maskMachineIpsInText(text, machineHosts, false);
    expect(unmasked).toBe(text);
  });

  it("masks SSH MOTD banners containing interface IPv4 and login IPs", () => {
    const motd = `System load: 0.87 Processes: 251
Usage of /: 60.3% of 231.44GB Users logged in: 1
Memory usage: 46% IPv4 address for ens6: 212.227.52.118
Last login: Sun Aug 16 21:46:46 2026 from 93.151.225.49`;

    const masked = maskAllIpsInText(motd);
    expect(masked).toContain("IPv4 address for ens6: 212.***.***.118");
    expect(masked).toContain("from 93.***.***.49");
    expect(masked).not.toContain("212.227.52.118");
    expect(masked).not.toContain("93.151.225.49");
  });

  it("masks terminal data bytes seamlessly", () => {
    const raw = "IPv4 address for ens6: 212.227.52.118";
    const bytes = new TextEncoder().encode(raw);
    const result = maskTerminalData(bytes, [], true);
    expect(result).toBe("IPv4 address for ens6: 212.***.***.118");
  });
});

