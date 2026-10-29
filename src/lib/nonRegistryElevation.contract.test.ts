import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = (path: string) => readFileSync(path, "utf8");

describe("non-registry machine-control elevation fallback", () => {
  test("guards the requested privileged surfaces before their write commands", () => {
    const surfaces = [
      ["src/panels/network/index.tsx", "setSecureDNS"],
      ["src/panels/network/WifiGuardSection.tsx", "patchAppSettings"],
      ["src/panels/mesh/VpnKillSwitchSection.tsx", "vpn_kill_switch_arm"],
      ["src/panels/privacy/DriverHealthSection.tsx", "start_driver_watch"],
      ["src/panels/apps/components/AppInstallerPanel.tsx", "installWingetApps"],
      ["src/panels/server-apps/ManageAppsDialog.tsx", "await onSave"],
      ["src/panels/secret/BrandingLicensingSection.tsx", "renameComputer"],
      ["src/panels/secret/index.tsx", "hide_runtime_list"],
      ["src/components/RightSidebar.tsx", "full_lockdown"],
      ["src/panels/dashboard/index.tsx", "executeBackendCommand"],
    ] as const;

    const wifi = source("src/panels/network/WifiGuardSection.tsx");
    const wifiMonitor = source("src/hooks/useWifiGuardMonitor.ts").replace(/\r\n/g, "\n");
    expect(wifi).toContain("const toggle = (next: boolean) => mutate(async () =>");
    expect(wifi).toContain("wifiGuard: { enabled: next }");
    expect(wifiMonitor).toContain("const authoritative = await invoke<AppSettings>('get_settings')");
    expect(wifiMonitor).toContain("if (!saved?.enabled)");
    expect(wifiMonitor).toContain("await invoke('start_wifi_guard')");
    const guardedMutation = wifi.slice(wifi.indexOf("  const mutate ="), wifi.indexOf("  const policy ="));
    expect(guardedMutation.indexOf("if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }")).toBeGreaterThanOrEqual(0);
    expect(guardedMutation.indexOf("if (needsElevation)")).toBeLessThan(guardedMutation.indexOf("await withWifiGuardOperation(operation)"));
    const reconcile = wifiMonitor.slice(wifiMonitor.indexOf("    const reconcile ="), wifiMonitor.indexOf("    void reconcile();"));
    const read = reconcile.indexOf("const authoritative = await invoke<AppSettings>('get_settings')");
    const disabled = reconcile.indexOf("if (!saved?.enabled)");
    const start = reconcile.indexOf("await invoke('start_wifi_guard')");
    expect(read).toBeGreaterThanOrEqual(0);
    expect(read).toBeLessThan(disabled);
    expect(disabled).toBeLessThan(start);
    expect(reconcile.slice(disabled, start)).toContain("stopConfirmed = true;\n            return;");

    for (const [path, dispatch] of surfaces) {
      const text = source(path);
      expect(text).toContain("isPrivilegedWriteBlocked");
      expect(text).toContain("MACHINE_SCOPE_ELEVATION_MESSAGE");
      expect(text).toContain("if (needsElevation)");
      expect(text).toContain(dispatch);
    }
  });
});
