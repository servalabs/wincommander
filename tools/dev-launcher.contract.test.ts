import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as {
  scripts: Record<string, string>;
};
const devLauncher = readFileSync("tools/dev.ps1", "utf8");
const combinedLauncher = readFileSync("tools/dev-all.ts", "utf8");
const serviceSync = readFileSync("tools/sync-dev-service.ps1", "utf8");

describe("desktop development launchers", () => {
  test("excludes locked runtime files from the frontend watcher", () => {
    const viteConfig = readFileSync("vite.config.ts", "utf8");
    expect(viteConfig).toMatch(/watch:\s*\{\s*ignored:\s*\[[\s\S]*?"\*\*\/\.dev\/\*\*"/);
  });
  test("bounds service shutdown before replacing the running development binary", () => {
    expect(serviceSync).toContain("Stop-Service -Name $serviceName -Force -NoWait -ErrorAction Stop");
    const stopped = "WaitForStatus('Stopped', [TimeSpan]::FromSeconds(150))";
    expect(serviceSync).toContain(stopped);
    expect(serviceSync.indexOf(stopped)).toBeLessThan(
      serviceSync.indexOf("Copy-Item -LiteralPath $builtService -Destination $stagedService"),
    );
    expect(serviceSync).not.toMatch(/Stop-Process|taskkill/i);
  });

  test.skipIf(process.platform !== "win32")("accepts a matching driver path in multiline service output and rejects a missing path", () => {
    // Parse only the readiness conditions; never execute the service installer.
    const script = String.raw`
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
  (Join-Path (Get-Location) 'tools/sync-dev-service.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'PowerShell parse failed' }
$conditions = @($ast.FindAll({ param($node)
  $node -is [System.Management.Automation.Language.IfStatementAst]
}, $true) | ForEach-Object { $_.Clauses.Item1.Extent.Text } | Where-Object {
  $_ -match '\$(driverConfig|config)\s+-(notmatch|match)'
})
if ($conditions.Count -ne 2) { throw 'Expected both driver path checks' }
$driverNtPath = '\??\C:\Program Files\WinCommander\driver.sys'
$driverRunning = $true
$LASTEXITCODE = 0
function Get-DriverImagePath { return $driverNtPath }
$results = foreach ($pathMatches in @($true, $false)) {
  $image = if ($pathMatches) { $driverNtPath } else { '\??\C:\wrong.sys' }
  $config = @('[SC] QueryServiceConfig SUCCESS', 'SERVICE_NAME: WinCommanderEncVol',
    'TYPE : 1 KERNEL_DRIVER', "BINARY_PATH_NAME : $image", 'START_TYPE : 3 DEMAND_START')
  $driverConfig = $config
  foreach ($condition in $conditions) {
    [bool](& ([scriptblock]::Create($condition)))
  }
}
ConvertTo-Json -Compress -InputObject @($results)
`;
    const result = spawnSync("powershell.exe", [
      "-NoProfile", "-NonInteractive", "-EncodedCommand",
      Buffer.from(script, "utf16le").toString("base64"),
    ], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(JSON.parse(result.stdout.trim())).toEqual([false, false, true, true]);
  });

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
    expect(serviceSync).toContain('Test-CompatibleVeraCryptDriverReady');
    expect(serviceSync).toContain("Using the already-running compatible VeraCrypt driver.");
    expect(serviceSync).toContain("reg.exe add $driverRegistryPath '/v' 'ImagePath' '/t' 'REG_EXPAND_SZ'");
    expect(serviceSync).toContain('(Get-DriverImagePath) -cne $driverNtPath');
  });
});
