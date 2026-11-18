import { describe, expect, test } from "bun:test";
import { dnsLabel } from "./RadarControlStrip";
import type { DNSStatus } from "../../hooks/useBackend";

const status = (patch: Partial<DNSStatus>): DNSStatus => ({ servers: [], adapter: null, provider: null, ...patch });

describe("dashboard DNS readout", () => {
  test("keeps DNS as the category and shows Enterprise DNS for identified NextDNS", () => {
    expect(dnsLabel(status({ provider: "Swiss_Firewall" }))).toBe("Enterprise DNS");
    expect(dnsLabel(status({ resolverOrg: "NextDNS" }))).toBe("Enterprise DNS");
    expect(dnsLabel(status({ dohTemplate: "https://dns.nextdns.io/example" }))).toBe("Enterprise DNS");
  });

  test("keeps unknown and other resolvers distinct", () => {
    expect(dnsLabel(null)).toBe("checking…");
    expect(dnsLabel(status({}))).toBe("—");
    expect(dnsLabel(status({ servers: ["192.0.2.1"] }))).toBe("Configured");
    expect(dnsLabel(status({ provider: "ControlD" }))).toBe("Simple Firewall");
  });
});
