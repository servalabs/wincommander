import { describe, expect, test } from "bun:test";
import { getToggleById } from "./index";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

describe("App Diagnostics permission enforcement", () => {
  test("uses the force-deny Windows policy and probes its effective state", async () => {
    const toggle = getToggleById("cap-appDiagnostics");
    expect(toggle?.needsAdmin).toBe(true);
    expect(toggle?.defaultOn).toBe(true);
    expect(toggle?.capabilityKey).toBe("appDiagnostics");
    expect(toggle?.checkedWhen).toBe("Deny");
    expect(toggle?.description).toContain("separate from Windows ETW tracing");
    expect(getToggleById("diagTracing")?.description).toContain("App Diagnostics access is controlled separately");

    for (const path of [
      "src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1",
      "src-tauri/wincmd-shared/scripts/capability-access.ps1",
    ]) {
      const source = await Bun.file(path).text();
      expect(source).toContain("'appDiagnostics' = 'LetAppsGetDiagnosticInfo'");
      expect(source).toContain("$policyValue -eq 2");
      expect(source).toContain("$effective = Get-AppCapabilityAccessStatus -Capability $Capability");
      expect(source).toContain("if ($effective.error)");
      expect(source).toContain("disabled = $null");
    }
  });

  test("keeps App Diagnostics permission errors unknown instead of reporting Allow", async () => {
    const source = await Bun.file("src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1").text();
    expect(source).toContain("$results[$cap] = if ($state.error) { $null } else { [bool]$state.disabled }");
    expect(source).toContain("requestedValue = $Access");
    expect(source).toContain("value          = $null");
  });
});
