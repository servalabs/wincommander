import { describe, expect, it } from "bun:test";
import {
  arpMacKey,
  arpRowKey,
  classifyArpError,
  compareIpv4,
  describeArpClear,
  findSuspiciousMacs,
  formatArpTableForClipboard,
  isGroupMac,
  sortArpEntries,
  summarizeArpScan,
} from "./arpDiagnostics";
import type { ArpEntry, ArpScan } from "../hooks/useBackend";

const entry = (over: Partial<ArpEntry>): ArpEntry => ({
  interface: "192.168.1.10 --- 0x5",
  address: "192.168.1.1",
  physicalAddress: "aa-bb-cc-dd-ee-01",
  entryType: "dynamic",
  ...over,
});

const scan = (entries: ArpEntry[]): ArpScan => ({
  scanId: "s1",
  entries,
  dynamicEntries: entries.filter((e) => e.entryType === "dynamic").length,
});

describe("compareIpv4", () => {
  it("orders octets numerically so .9 comes before .10", () => {
    expect(compareIpv4("10.0.0.9", "10.0.0.10") < 0).toBe(true);
  });

  it("falls back to string order for non-dotted-quad input", () => {
    expect(compareIpv4("fe80::1", "fe80::2") < 0).toBe(true);
  });
});

describe("isGroupMac", () => {
  it("flags the IPv4 multicast prefix", () => {
    expect(isGroupMac("01-00-5e-00-00-16")).toBe(true);
  });
  it("flags broadcast", () => {
    expect(isGroupMac("ff-ff-ff-ff-ff-ff")).toBe(true);
  });
  it("does not flag a normal unicast MAC", () => {
    expect(isGroupMac("aa-bb-cc-dd-ee-01")).toBe(false);
  });
  it("tolerates colon separators and empty input", () => {
    expect(isGroupMac("aa:bb:cc:dd:ee:01")).toBe(false);
    expect(isGroupMac("")).toBe(false);
  });
});

describe("sortArpEntries", () => {
  it("sorts by IP numerically by default", () => {
    const sorted = sortArpEntries([
      entry({ address: "192.168.1.100" }),
      entry({ address: "192.168.1.2" }),
      entry({ address: "192.168.1.20" }),
    ]);
    expect(sorted.map((e) => e.address)).toEqual([
      "192.168.1.2",
      "192.168.1.20",
      "192.168.1.100",
    ]);
  });

  it("does not mutate the input array", () => {
    const input = [entry({ address: "192.168.1.5" }), entry({ address: "192.168.1.1" })];
    sortArpEntries(input);
    expect(input[0].address).toBe("192.168.1.5");
  });

  it("reverses on desc", () => {
    const sorted = sortArpEntries(
      [entry({ address: "10.0.0.1" }), entry({ address: "10.0.0.9" })],
      "address",
      "desc",
    );
    expect(sorted[0].address).toBe("10.0.0.9");
  });
});

describe("findSuspiciousMacs", () => {
  it("flags one MAC answering for two IPs on the same adapter", () => {
    const flagged = findSuspiciousMacs([
      entry({ address: "192.168.1.1", physicalAddress: "aa-bb-cc-00-00-01" }),
      entry({ address: "192.168.1.2", physicalAddress: "aa-bb-cc-00-00-01" }),
    ]);
    expect(flagged.has(arpMacKey(entry({ physicalAddress: "aa-bb-cc-00-00-01" })))).toBe(true);
  });

  it("does not flag the same MAC on two different adapters", () => {
    const flagged = findSuspiciousMacs([
      entry({ interface: "a", address: "192.168.1.1", physicalAddress: "aa-bb-cc-00-00-01" }),
      entry({ interface: "b", address: "192.168.1.2", physicalAddress: "aa-bb-cc-00-00-01" }),
    ]);
    expect(flagged.size).toBe(0);
  });

  it("never flags multicast plumbing rows", () => {
    const flagged = findSuspiciousMacs([
      entry({ address: "224.0.0.22", physicalAddress: "01-00-5e-00-00-16", entryType: "static" }),
      entry({ address: "239.255.255.250", physicalAddress: "01-00-5e-00-00-16", entryType: "static" }),
    ]);
    expect(flagged.size).toBe(0);
  });

  it("treats separator style as the same MAC", () => {
    const flagged = findSuspiciousMacs([
      entry({ address: "192.168.1.1", physicalAddress: "aa-bb-cc-00-00-01" }),
      entry({ address: "192.168.1.2", physicalAddress: "AA:BB:CC:00:00:01" }),
    ]);
    expect(flagged.size).toBe(1);
  });
});

describe("summarizeArpScan", () => {
  it("explains an empty cache instead of showing a bare count", () => {
    expect(summarizeArpScan(scan([])).headline).toBe("Nothing cached yet");
  });

  it("warns when a MAC answers for several IPs", () => {
    const summary = summarizeArpScan(
      scan([
        entry({ address: "192.168.1.1", physicalAddress: "aa-bb-cc-00-00-01" }),
        entry({ address: "192.168.1.2", physicalAddress: "aa-bb-cc-00-00-01" }),
      ]),
    );
    expect(summary.intent).toBe("warning");
    expect(summary.detail).toContain("ARP spoofing");
  });

  it("reports an all-static cache as nothing to do", () => {
    const summary = summarizeArpScan(scan([entry({ entryType: "static" })]));
    expect(summary.intent).toBe("success");
    expect(summary.headline).toContain("static");
  });

  it("calls a normal mixed cache healthy and says when clearing helps", () => {
    const summary = summarizeArpScan(
      scan([
        entry({ address: "192.168.1.1" }),
        entry({ address: "192.168.1.2", physicalAddress: "aa-bb-cc-00-00-02" }),
        entry({ address: "192.168.1.255", physicalAddress: "ff-ff-ff-ff-ff-ff", entryType: "static" }),
      ]),
    );
    expect(summary.intent).toBe("success");
    expect(summary.headline).toContain("2 of 3");
  });
});

describe("classifyArpError", () => {
  it("turns the Administrator refusal into an elevation hint", () => {
    const advice = classifyArpError("arp.exe refused to clear the cache; Administrator may be required");
    expect(advice.needsElevation).toBe(true);
    expect(advice.hint).toContain("Administrator");
  });

  it("maps the expired-scan error to a rescan instruction without elevation", () => {
    const advice = classifyArpError("ARP scan expired or is invalid; scan again");
    expect(advice.needsElevation).toBe(false);
    expect(advice.hint).toContain("10 minutes");
  });

  it("explains the decoy-mode refusal", () => {
    expect(classifyArpError("Refused: ARP maintenance is unavailable in Decoy mode.").title).toContain(
      "Decoy",
    );
  });

  it("explains the investigator-mode refusal", () => {
    expect(
      classifyArpError("Refused: investigator mode forbids ARP cache mutation.").title,
    ).toContain("investigator");
  });

  it("keeps an unknown error visible instead of swallowing it", () => {
    expect(classifyArpError("something odd happened").hint).toBe("something odd happened");
  });
});

describe("describeArpClear", () => {
  it("renders the before denominator that used to be dropped", () => {
    expect(describeArpClear({ before: 12, remaining: 0, cleared: 12 })).toContain("12 of 12");
  });

  it("explains re-learned entries rather than looking like a failure", () => {
    expect(describeArpClear({ before: 12, remaining: 2, cleared: 10 })).toContain("re-learned");
  });

  it("handles a cache that had nothing to clear", () => {
    expect(describeArpClear({ before: 0, remaining: 0, cleared: 0 })).toContain("no learned entries");
  });
});

describe("formatArpTableForClipboard", () => {
  it("emits a tab-separated table with a header row", () => {
    const text = formatArpTableForClipboard([entry({})]);
    const [header, row] = text.split("\n");
    expect(header.split("\t")).toEqual(["IP ADDRESS", "MAC ADDRESS", "ADAPTER", "TYPE"]);
    expect(row.split("\t")[0]).toBe("192.168.1.1");
  });
});

describe("arpRowKey", () => {
  it("is unique per adapter+address so rescans reconcile correctly", () => {
    expect(arpRowKey(entry({ interface: "a" }))).not.toBe(arpRowKey(entry({ interface: "b" })));
  });
});
