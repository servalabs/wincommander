import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("maintenance security and hygiene result handling", () => {
  test("starts the read-only storage schedule warm-up ten seconds after startup", async () => {
    const pollers = await Bun.file("src/components/BackgroundPollers.tsx").text();
    const storage = await Bun.file("src/components/tweaks/managers/DiskCleanupGranular.tsx").text();

    expect(pollers).toContain("preloadDiskCleanupScheduleStatus({");
    expect(pollers).toContain("}, 10_000)");
    expect(storage).toContain("loadDiskCleanupSchedules(getAutoEraseSchedules)");
    expect(storage).toContain("subscribeToDiskCleanupScheduleInvalidation");
  });

  test("does not expose a Security Center tab in Maintenance", async () => {
    const maintenance = await Bun.file("src/panels/maintenance/index.tsx").text();

    expect(maintenance).not.toContain('TabsTrigger value="security"');
    expect(maintenance).not.toContain("<SecurityCenter />");
  });

  test("shows exact shortcut paths with wrapping and full-path actions", async () => {
    const hygiene = await Bun.file("src/panels/maintenance/SystemHygieneTools.tsx").text();

    expect(hygiene).toContain('className="block whitespace-pre-wrap [overflow-wrap:anywhere]">{path}</span>');
    expect(hygiene).toContain("navigator.clipboard.writeText(path)");
    expect(hygiene).toContain("Open containing folder in Explorer: ${path}");
    expect(hygiene).toContain("Windows does not prove");
  });

  test("renders Defender scan results and exposes the Windows Security fallback", async () => {
    const malware = await Bun.file("src/panels/maintenance/MalwareCenter.tsx").text();

    expect(malware).toContain('aria-label="Open Windows Security"');
    expect(malware).toContain("openWindowsSecuritySettings()");
    expect(malware).toContain("Windows did not open Security settings");
    expect(malware).toContain("windowsSecurityError");
    expect(malware).toContain("center.scan.findings.map");
    expect(malware).toContain("currentScanNotice.message");
  });

  test("keeps successful security sources visible when another source fails", async () => {
    const securityData = await Bun.file("src/panels/maintenance/SecurityData.tsx").text();

    expect(securityData).toContain("Promise.allSettled([");
    expect(securityData).toContain("setThreat(threatResult.value)");
    expect(securityData).toContain("setCve(cveResult.value)");
    expect(securityData).toContain("Local threat posture");
    expect(securityData).toContain("Windows CVE coverage");
    expect(securityData).toContain("Unavailable counts do not mean zero detections");
  });
});
