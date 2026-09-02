import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("incoming RDP lock control", () => {
  test("uses the machine-only typed service operation and verifies its observed state", async () => {
    const source = await Bun.file("src/panels/privacy/RemoteAccessMonitorSection.tsx").text();

    expect(source).toContain('setting: "rdp_lock"');
    expect(source).toContain('value: { kind: "rdp_lock", locked }');
    expect(source).toContain('observed.kind === "rdp_lock" && observed.locked === locked');
    expect(source).toContain("service read-back did not match the requested RDP lock state");
    expect(source).toContain("Machine only");
  });

  test("blocks standard users before the machine setting is dispatched", async () => {
    const source = await Bun.file("src/panels/privacy/RemoteAccessMonitorSection.tsx").text();

    expect(source).toContain("Blocked: needs-elevation");
    expect(source).toContain("Requires an administrator");
    expect(source).toContain("disabled={applyingRdpLock || needsElevation}");
  });
});
