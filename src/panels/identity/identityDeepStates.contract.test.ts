import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const identity = readFileSync("src/panels/identity/index.tsx", "utf8");
const license = readFileSync("src/panels/identity/components/AppLicensePanel.tsx", "utf8");
const quickLicense = readFileSync("src/components/LicenseQuickPanel.tsx", "utf8");

describe("settings deep-state contracts", () => {
  test("a rejected censorship apply clears busy state and reports failure", () => {
    expect(identity).toContain("setStatus('failed')");
    expect(identity).toContain("setCensorshipBusy(false)");
    expect(identity).toContain('className="identity-censorship-error" role="alert"');
  });

  test("license key label is programmatically associated", () => {
    expect(license).toContain('labelFor="identity-license-key"');
    expect(license).toContain('id="identity-license-key"');
  });

  test("keeps shared-device licence removal behind an elevated administrator token", () => {
    for (const source of [license, quickLicense]) {
      expect(source).toContain('invoke<boolean>("is_current_process_elevated")');
      expect(source).toContain("canRemoveDeviceLicense");
      expect(source).toContain("Administrator approval is required to remove this device license");
    }
    expect(license).toContain("disabled={!canRemoveDeviceLicense}");
    expect(quickLicense).toContain("disabled={loading || !canRemoveDeviceLicense}");
  });

  test("Fix All's all-users preference remains a per-user opt-in", () => {
    expect(identity).toContain("Apply supported Fix All protections to all users");
    expect(identity).toContain("User-specific preferences still apply only to the signed-in Windows user");
    expect(identity).toContain("appSettings?.app?.applyFixAllMachineWide === true");
    expect(identity).toContain("patchAppSettings({ app: { applyFixAllMachineWide: enabled } })");
  });

  test("keeps automatic Fix All beside the update preference in Settings", () => {
    const updateToggle = identity.indexOf("Automatically update installed apps");
    const autoFixToggle = identity.indexOf("Automatically apply safe Fix All recommendations");

    expect(updateToggle).toBeGreaterThan(-1);
    expect(autoFixToggle).toBeGreaterThan(updateToggle);
    expect(identity).toContain("appSettings?.app?.autoFixAll === true");
    expect(identity).toContain("patchAppSettings({ app: { autoFixAll: enabled } })");
  });
});
