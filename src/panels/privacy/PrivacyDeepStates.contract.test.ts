import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Privacy deep-state and responsive contracts", () => {
  test("monitor columns collapse from their real container width", async () => {
    const [panel, css] = await Promise.all([
      Bun.file("src/panels/privacy/index.tsx").text(),
      Bun.file("src/panels/privacy/index.css").text(),
    ]);
    expect(panel).toContain('className="privacy-monitoring-shell"');
    expect(css).toContain(".privacy-monitoring-shell");
    expect(css).toContain("container-type: inline-size");
    expect(css).toContain("@container (max-width: 760px)");
  });

  test("Argus cards distinguish checking, error, on, and off", async () => {
    for (const file of [
      "ArgusAppUsageSection.tsx",
      "ArgusDlpSection.tsx",
      "ArgusPrintUsbSection.tsx",
      "ArgusTamperSection.tsx",
    ]) {
      const source = await Bun.file(`src/panels/privacy/${file}`).text();
      expect(source).toContain("const statusLoading = status === null && monitorError === null");
      expect(source).toContain("CHECKING");
      expect(source).toContain("ERROR");
      expect(source).toContain("setMonitorError(null)");
      expect(source).toContain('role="alert"');
    }
  });

  test("visible Privacy errors are announced", async () => {
    for (const file of [
      "AIRuntimeInstaller.tsx",
      "AuthAnomalySection.tsx",
      "BrowserHardeningSection.tsx",
      "CanaryTokensSection.tsx",
      "CheckInTimerSection.tsx",
      "DriverHealthSection.tsx",
      "FileWatchTriggerSection.tsx",
      "MonitoringMirrorSection.tsx",
      "PrintActivitySection.tsx",
      "RdpIdleCard.tsx",
      "ScreenCaptureSection.tsx",
      "SessionAssuranceSection.tsx",
      "UsbDevicesSection.tsx",
    ]) {
      expect(await Bun.file(`src/panels/privacy/${file}`).text()).toContain('role="alert"');
    }
  });

  test("Privacy Shield disclosure is keyboard-native and container-safe", async () => {
    const source = await Bun.file("src/panels/privacy/PrivacyShieldCard.tsx").text();
    expect(source).toContain('<button type="button" className="flex w-full items-center justify-between');
    expect(source).toContain('aria-expanded={showAdvanced}');
    expect(source).toContain('aria-controls="privacy-shield-processing-parameters"');
    expect(source).toContain("repeat(auto-fit,minmax(min(100%,160px),1fr))");
    expect(source).toContain('aria-label="How Privacy Gaze Shield works"');
    expect(source).toContain('aria-label="Upgrade Privacy Shield for unlimited time"');
  });

  test("Pro upsell badges are keyboard-native actions", async () => {
    const source = await Bun.file("src/panels/privacy/RdpIdleCard.tsx").text();
    expect(source).toContain('<button type="button" aria-label="Upgrade to unlock Idle Session Monitor"');
    expect(source).toContain('<button type="button" aria-label="Upgrade to unlock Incoming Idle Sign-Out"');
  });
});
