import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const packageJson = JSON.parse(readFileSync("package.json", "utf8")) as {
  scripts: Record<string, string>;
};
const devLauncher = readFileSync("tools/dev.ps1", "utf8");
const combinedLauncher = readFileSync("tools/dev-all.ts", "utf8");
const serviceSync = readFileSync("tools/sync-dev-service.ps1", "utf8");
const devServer = readFileSync("tools/dev-server.ts", "utf8");

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
    expect(devLauncher).not.toContain('sync-dev-service.ps1');
    expect(devServer).toContain('"tools/sync-dev-service.ps1", "-SyncPro"');
    expect(devServer.indexOf('"tools/build-pro.ts"')).toBeLessThan(devServer.indexOf('"tools/sync-dev-service.ps1"'));
    expect(devServer.indexOf('"tools/sync-dev-service.ps1"')).toBeLessThan(devServer.indexOf('const vite = spawn'));
    expect(devServer).toContain('if (serviceResult !== 0)');
    const tauri = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8"));
    expect(tauri.build.beforeDevCommand).toContain('dev:server');
    expect(devLauncher).toContain('& $bun run dev:server');
  });

  test("verifies the running service process and current Pro helper", () => {
    expect(serviceSync).toContain('Test-ServiceProcessCurrent');
    expect(serviceSync).toContain('$runningProcess.ExecutablePath.Equals($stagedService');
    expect(serviceSync).toContain('(-not $SyncPro -or (Test-DevelopmentProCurrent))');
    expect(serviceSync).toContain('$existingBuildArgument$proArgument');
    expect(serviceSync).toContain('The service Pro helper does not match the current development build.');
  });

  test.skipIf(process.platform !== "win32")("missing services are false rather than StrictMode exceptions", () => {
    const expression = serviceSync.match(/\$serviceRunning = ([^\r\n]+)/)?.[1];
    expect(expression).toBeDefined();
    const script = `Set-StrictMode -Version Latest; $values = foreach ($service in @($null, [pscustomobject]@{Status='Stopped'}, [pscustomobject]@{Status='Running'})) { ${expression} }; ConvertTo-Json -Compress -InputObject @($values)`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(script, "utf16le").toString("base64")], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(JSON.parse(result.stdout.trim())).toEqual([false, false, true]);
  });

  test.skipIf(process.platform !== "win32")("only the verified parent tolerates an unreadable SYSTEM process path", () => {
    const script = String.raw`
Set-StrictMode -Version Latest
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
  (Join-Path (Get-Location) 'tools/sync-dev-service.ps1'), [ref]$null, [ref]$null)
$definition = $ast.Find({ param($node)
  $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -eq 'Test-ServiceProcessCurrent'
}, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$stagedService = 'C:\checkout\.dev\wincommander-svc.exe'
function Get-CimInstance {
  [CmdletBinding()] param([string]$ClassName, [string]$Filter)
  if ($ClassName -eq 'Win32_Service') { return [pscustomobject]@{ProcessId=42} }
  return [pscustomobject]@{ExecutablePath=$script:observedImage}
}
$results = foreach ($script:observedImage in @($null, 'C:\old\wincommander-svc.exe', $stagedService)) {
  Test-ServiceProcessCurrent
  Test-ServiceProcessCurrent -AllowUnverifiable
}
ConvertTo-Json -Compress -InputObject @($results)
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(script, "utf16le").toString("base64")], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(JSON.parse(result.stdout.trim())).toEqual([false, true, false, false, true, true]);
    expect(serviceSync).toContain('-AllowUnverifiable:(-not $Elevated)');
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
