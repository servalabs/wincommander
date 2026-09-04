import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as {
  scripts: Record<string, string>;
};
const devLauncher = readFileSync("tools/dev.ps1", "utf8");
const combinedLauncher = readFileSync("tools/dev-all.ts", "utf8");
const serviceSync = readFileSync("tools/sync-dev-service.ps1", "utf8");

describe("desktop development launchers", () => {
  test("synchronizes the Vault service before starting the normal desktop", () => {
    expect(packageJson.scripts["dev:tauri"]).toContain("tools/dev.ps1");
    expect(devLauncher).toContain('sync-dev-service.ps1');
    expect(devLauncher.indexOf('sync-dev-service.ps1')).toBeLessThan(
      devLauncher.indexOf('x tauri dev'),
    );
  });

  test("routes the combined developer launcher through the normal desktop launcher", () => {
    expect(combinedLauncher).toContain('"run", "dev:tauri"');
    expect(combinedLauncher).not.toContain('"x", "tauri", "dev"');
  });

  test("does not repeat Cargo work after the parent has prepared the service binary", () => {
    expect(serviceSync).toContain("[switch]$UseExistingBuild");
    expect(serviceSync).toContain("Start-ElevatedSync -UseExistingBuild");
    expect(serviceSync).toContain("Using the development service built by the non-elevated parent.");
  });

  test("keeps a spaced development service path quoted for the Windows service manager", () => {
    expect(serviceSync).toContain(`$servicePathArgument = '"' + $stagedService + '"'`);
    expect(serviceSync).not.toContain(`$servicePathArgument = '\\"' + $stagedService + '\\"'`);
  });

  test("repairs the fixed encrypted-volume driver when its pinned payload is present", () => {
    expect(serviceSync).toContain("$driverServiceName = 'WinCommanderEncVol'");
    expect(serviceSync).toContain("$driverSha256 = '1F0C6DB3559D1356C38A1486A967CD90DB5E6202E433FEA1DFE510DDB884FFB6'");
    expect(serviceSync).toContain('Ensure-EncryptedVolumeDriver');
    expect(serviceSync).toContain('Test-EncryptedVolumeDriverReady');
    expect(serviceSync).toContain("reg.exe add $driverRegistryPath '/v' 'ImagePath' '/t' 'REG_EXPAND_SZ'");
    expect(serviceSync).toContain('(Get-DriverImagePath) -cne $driverNtPath');
  });
});
