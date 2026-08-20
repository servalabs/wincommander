import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Privacy Monitor control accessibility", () => {
  test("monitor surface theming does not override checked checkbox colors", async () => {
    const css = await Bun.file("src/panels/privacy/index.css").text();

    expect(css).toContain(
      '[class*="bg-[var(--surface-2)]"]:not([role="checkbox"])',
    );
    expect(css).not.toContain(
      '[class*="bg-[var(--surface-2)]"] {',
    );
  });

  test("labels monitor switches, disclosure buttons, and clear actions", async () => {
    const [ransomware, decoy, rdp, usb, canary, shield, screenCapture, remoteAccess, paste, bp, slider, printAudit, dlp, printUsb, tamper, lockdown, rightSidebar] = await Promise.all([
      Bun.file("src/panels/privacy/RansomwareMonitorSection.tsx").text(),
      Bun.file("src/panels/privacy/DecoyMonitorSection.tsx").text(),
      Bun.file("src/panels/privacy/RdpIdleCard.tsx").text(),
      Bun.file("src/panels/privacy/UsbDevicesSection.tsx").text(),
      Bun.file("src/panels/privacy/CanaryTokensSection.tsx").text(),
      Bun.file("src/panels/privacy/PrivacyShieldCard.tsx").text(),
      Bun.file("src/panels/privacy/ScreenCaptureSection.tsx").text(),
      Bun.file("src/panels/privacy/RemoteAccessMonitorSection.tsx").text(),
      Bun.file("src/panels/privacy/PasteMonitorSection.tsx").text(),
      Bun.file("src/components/ui/bp.tsx").text(),
      Bun.file("src/components/ui/slider.tsx").text(),
      Bun.file("src/panels/privacy/PrintActivitySection.tsx").text(),
      Bun.file("src/panels/privacy/ArgusDlpSection.tsx").text(),
      Bun.file("src/panels/privacy/ArgusPrintUsbSection.tsx").text(),
      Bun.file("src/panels/privacy/ArgusTamperSection.tsx").text(),
      Bun.file("src/panels/privacy/LockdownWordsSection.tsx").text(),
      Bun.file("src/components/RightSidebar.tsx").text(),
    ]);

    expect(ransomware).toContain('aria-label="Enable mass-encryption sentinel"');
    expect(ransomware).toContain('aria-label="Configure mass-encryption sentinel"');
    expect(ransomware).toContain('aria-label="Clear mass-encryption alerts"');
    expect(ransomware).toContain("aria-expanded={expanded}");
    expect(ransomware).toContain('ariaLabel="Mass-encryption file threshold"');
    expect(ransomware).toContain('ariaLabel="Mass-encryption detection window in seconds"');
    expect(ransomware).toContain("aria-pressed={active}");
    expect(ransomware).toContain('aria-label="How mass-encryption detection works"');

    expect(decoy).toContain('aria-label="Enable decoy file monitor"');
    expect(decoy).toContain('aria-label="Configure decoy file monitor"');
    expect(decoy).toContain('aria-label="Clear decoy access events"');
    expect(decoy).toContain("aria-expanded={expanded}");
    expect(decoy).toContain('aria-label="How decoy file monitoring works"');

    expect(rdp).toContain('aria-label="Close outgoing RDP sessions after idle timeout"');
    expect(rdp).toContain('aria-label="Sign out idle incoming RDP sessions"');
    expect(rdp).toContain('aria-label="Outgoing RDP idle timeout"');
    expect(rdp).toContain('aria-label="Outgoing RDP warning countdown"');

    expect(usb).toContain('aria-label="Clear USB device timeline"');
    expect(usb).toContain("aria-label={`Block ${name}`}");
    expect(usb).toContain("aria-label={`Allow ${name}`}");
    expect(usb).toContain("aria-label={`Make ${name} read-only`}");
    expect(usb).toContain('aria-label="Clear USB auto-isolate actions"');
    expect(usb).toContain("aria-pressed={autoSandboxMode === m}");
    expect(usb).toContain('aria-label="Refresh USB device timeline"');
    expect(usb).toContain('aria-label="Clear BadUSB alerts"');
    expect(usb).toContain('role="group" aria-label="USB auto-isolate mode"');

    expect(canary).toContain('aria-label="Canary token type"');
    expect(canary).toContain('aria-label="Clear canary hit history"');
    expect(canary).toContain('aria-label="Stop canary listener"');
    expect(canary).toContain('aria-label="Start canary listener"');
    expect(canary).toContain('aria-label="Refresh canary tokens"');

    expect(shield).toContain("aria-label={label}");
    expect(shield).toContain("ariaLabel={label}");
    expect(screenCapture).toContain('aria-label="Detect screen-capture tools"');
    expect(screenCapture).toContain('aria-label="Protect this window from capture"');
    expect(screenCapture).toContain('aria-label="Clear screen-capture detections"');

    expect(remoteAccess).toContain('aria-label="Enable remote access monitoring"');
    expect(remoteAccess).toContain('aria-label="Configure remote access monitor"');
    expect(remoteAccess).toContain("aria-label={`Watch ${t.label}`}");
    expect(remoteAccess).toContain('aria-label="Clear remote access detections"');

    expect(paste).toContain('aria-label="Enable clipboard secret guard"');
    expect(paste).toContain('aria-label="Configure clipboard secret guard"');
    expect(paste).toContain('aria-label="How clipboard secret monitoring works"');

    expect(bp).toContain('"aria-label": ariaLabel');
    expect(bp).toContain("aria-label={ariaLabel}");
    expect(bp).toContain("thumbAriaLabel={ariaLabel}");
    expect(slider).toContain('aria-label={typeof thumbAriaLabel === "function" ? thumbAriaLabel(index) : thumbAriaLabel}');

    expect(printAudit).toContain('aria-label="Refresh print audit events"');
    expect(dlp).toContain('aria-label="Refresh DLP signals"');
    expect(printUsb).toContain('aria-label="Refresh print and USB signals"');
    expect(tamper).toContain('aria-label="Refresh tamper events"');

    expect(lockdown).toContain('aria-label="Enable typed lockdown words"');
    expect(lockdown).toContain('aria-label="Configure typed lockdown words"');
    expect(lockdown).toContain("aria-expanded={expanded}");
    expect(lockdown).toContain("aria-label={`Remove ${p.label}`}");

    expect(rightSidebar).toContain('ariaLabel="Emergency dismount volumes and RAM disks"');
    expect(rightSidebar).toContain('ariaLabel="Quick mount encrypted volume"');
    expect(rightSidebar).toContain('ariaLabel="Open AI Security Advisor"');
    expect(rightSidebar).toContain('ariaLabel="Open instant file search"');
    expect(rightSidebar).toContain('ariaLabel="Open secure file and folder deletion"');
    expect(rightSidebar).toContain('ariaLabel="Open metadata scrubber"');
    expect(rightSidebar).toContain('ariaLabel={sdCountdown !== null ? "Abort lockdown countdown" : "Run configured lockdown"}');
  });
});
