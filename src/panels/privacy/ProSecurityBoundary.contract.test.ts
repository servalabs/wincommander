import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const root = decodeURIComponent(new URL("../../../", import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, "$1");
const source = (path: string) => readFileSync(`${root}/${path}`, "utf8");

describe("Pro security boundary", () => {
  test("advanced USB collectors and controls are entitlement-gated", () => {
    const usb = source("src/panels/privacy/UsbDevicesSection.tsx");
    expect(usb).toContain("const advancedAvailable = hasPaid && !entitlementLoading");
    expect(usb).toContain("if (!advancedAvailable) {");
    expect(usb).toContain('featureLabel="USB transfer metering"');
    expect(usb).toContain('featureLabel="USB HID anomaly alerts and auto-isolate"');
    expect(usb).toContain("low-confidence");
  });

  test("decoy monitor cannot issue filesystem watch calls without Pro", () => {
    const decoy = source("src/panels/privacy/DecoyMonitorSection.tsx");
    expect(decoy).toContain("if (!hasPaid) return;");
    expect(decoy).toContain("if (!hasPaid) {");
    expect(decoy).toContain('featureLabel="Decoy File Monitor"');
  });

  test("Free ransomware alarms omit PID response and Fleet controls", () => {
    const ransomware = source("src/panels/privacy/RansomwareMonitorSection.tsx");
    expect(ransomware).toContain("hasPaid\n          ? invoke<RansomwareMonitorHealth>");
    expect(ransomware).toContain('featureLabel="Ransomware process attribution"');
    expect(ransomware).toContain('featureLabel="Ransomware automatic response"');
    expect(ransomware).toContain('featureLabel="Fleet ransomware reporting"');
    expect(ransomware).toContain('columns={hasPaid ?');
  });
});
