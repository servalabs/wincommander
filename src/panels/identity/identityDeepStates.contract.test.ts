import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const identity = readFileSync("src/panels/identity/index.tsx", "utf8");
const license = readFileSync("src/panels/identity/components/AppLicensePanel.tsx", "utf8");

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
});
