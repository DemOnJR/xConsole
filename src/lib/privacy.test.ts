import { describe, expect, it } from "vitest";
import { maskIpString, maskMachineIpsInText } from "./privacy";

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
});
