import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = (path: string) => readFileSync(path, "utf8");

describe("non-registry machine-control elevation fallback", () => {
  test("guards the requested privileged surfaces before their write commands", () => {
    const surfaces = [
      ["src/panels/network/index.tsx", "setSecureDNS"],
      ["src/panels/network/WifiGuardSection.tsx", "start_wifi_guard"],
      ["src/panels/mesh/VpnKillSwitchSection.tsx", "vpn_kill_switch_arm"],
      ["src/panels/privacy/DriverHealthSection.tsx", "start_driver_watch"],
      ["src/panels/apps/components/AppInstallerPanel.tsx", "installWingetApps"],
      ["src/panels/server-apps/ManageAppsDialog.tsx", "await onSave"],
      ["src/panels/secret/BrandingLicensingSection.tsx", "renameComputer"],
      ["src/panels/secret/index.tsx", "hide_runtime_list"],
      ["src/components/RightSidebar.tsx", "full_lockdown"],
      ["src/panels/dashboard/index.tsx", "executeBackendCommand"],
    ] as const;

    for (const [path, dispatch] of surfaces) {
      const text = source(path);
      expect(text).toContain("isPrivilegedWriteBlocked");
      expect(text).toContain("MACHINE_SCOPE_ELEVATION_MESSAGE");
      expect(text).toContain("if (needsElevation)");
      expect(text).toContain(dispatch);
    }
  });
});
