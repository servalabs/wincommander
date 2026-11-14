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
const serviceStartup = readFileSync("tools/dev-service-startup.ts", "utf8");
const protectedRunner = readFileSync("tools/protected-dev-runner.ps1", "utf8");
const protectedRunnerCommand = readFileSync("tools/protected-dev-runner.cmd", "utf8");

function powershellStderrIsInformational(stderr: string): boolean {
  const text = stderr.trim();
  if (!text) return true;
  if (!text.startsWith("#< CLIXML")) return false;
  return !/\bS\s*=\s*["'](?:Error|Warning)["']|<(?:ErrorRecord|Exception)\b/i.test(text);
}

describe("desktop development launchers", () => {
  test("distinguishes PowerShell host/progress records from errors", () => {
    expect(powershellStderrIsInformational("")).toBe(true);
    expect(powershellStderrIsInformational('#< CLIXML\n<Objs><S S="information">message</S><PR /></Objs>')).toBe(true);
    expect(powershellStderrIsInformational('#< CLIXML\n<Objs><S S="Error">failure</S></Objs>')).toBe(false);
    expect(powershellStderrIsInformational('#< CLIXML\n<Objs><S S="Warning">warning</S></Objs>')).toBe(false);
    expect(powershellStderrIsInformational("plain error")).toBe(false);
  });
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

  test("synchronizes the settings service before starting either desktop edition", () => {
    expect(packageJson.scripts["dev:tauri"]).toContain("tools/dev.ps1");
    expect(packageJson.scripts["dev:admin"]).toContain("tools/dev.ps1 -Elevated");
    expect(devLauncher).not.toContain('sync-dev-service.ps1');
    expect(serviceStartup).toContain('"tools/sync-dev-service.ps1"');
    expect(serviceStartup).toContain('...(!freeOnly ? ["-SyncPro"] : [])');
    expect(devServer).toMatch(/if \(!FREE_ONLY\) \{[\s\S]*?buildProResult[\s\S]*?\n  \}\s*\/\/ Free[\s\S]*?await syncDevelopmentService\(FREE_ONLY/);
    expect(devServer.indexOf('"tools/build-pro.ts"')).toBeLessThan(devServer.indexOf('await syncDevelopmentService('));
    expect(devServer.indexOf('await syncDevelopmentService(')).toBeLessThan(devServer.indexOf('const vite = spawn'));
    expect(serviceStartup).toContain('if (result !== 0)');
    expect(serviceSync).toContain('(-not $SyncPro -or (Test-EncryptedVolumeDriverReady))');
    expect(serviceSync).toContain('if ($SyncPro) { Ensure-EncryptedVolumeDriver }');
    const tauri = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8"));
    expect(tauri.build.beforeDevCommand).toContain('dev:server');
    expect(devLauncher).toContain('& $bun run dev:server');
    expect(devLauncher).toContain('node_modules\\.bin\\tauri.exe');
    expect(packageJson.scripts.tauri).toContain(`@tauri-apps/cli@${packageJson.devDependencies['@tauri-apps/cli']}`);
  });

  test("clears the old desktop before setup and avoids killing the new one", () => {
    // Tauri starts Cargo while beforeDevCommand is still running. Cleanup must
    // happen before slow setup, and dev-server must not repeat it later.
    const tauri = JSON.parse(readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8"));
    expect(tauri.build.beforeDevCommand.indexOf("kill:dev")).toBeLessThan(
      tauri.build.beforeDevCommand.indexOf("setup:dev"),
    );
    expect(tauri.build.beforeDevCommand).toContain("dev:server --precleaned");
    expect(devServer).toContain('const PRE_CLEANED = process.argv.includes("--precleaned")');
    expect(devServer).toContain("...(policy.cleanProcesses ? [{");
    expect(devServer).toContain("if (policy.releasePort) freeVitePort()");
    expect(devServer).toContain("policy.noServiceRestart");
    // A fresh launch must not preserve a stale wincommander-pro.exe because
    // Cargo has to replace that exact debug binary before Vite is exposed.
    expect(devServer).toContain("developmentStartupPolicy({ preserveSessions: PRESERVE_WINCOMMANDER");
    expect(devServer).toContain('"tools/build-pro.ts"');
    expect(devServer).not.toContain('PRESERVE_WINCOMMANDER ? ["-PreserveWinCommander"]');
    expect(serviceSync).toContain("if ($NoServiceRestart -and $Elevated)");
  });

  test.skipIf(process.platform !== "win32")("preserve mode never inspects or stops processes or the listener", () => {
    const script = String.raw`
$ErrorActionPreference = 'Stop'
function Get-Process { throw 'unexpected process inspection' }
function Stop-Process { throw 'unexpected process termination' }
function netstat { throw 'unexpected listener inspection' }
& (Join-Path (Get-Location) 'tools/kill-dev.ps1') -PreserveWinCommander
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand",
      Buffer.from(script, "utf16le").toString("base64")], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("processes and frontend listener were preserved");
    expect(powershellStderrIsInformational(result.stderr)).toBe(true);
  });

  test.skipIf(process.platform !== "win32")("preserve service mode refuses an unstaged or changed service before mutation", () => {
    const script = String.raw`
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
  (Join-Path (Get-Location) 'tools/sync-dev-service.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'PowerShell parse failed' }
$node = $ast.FindAll({ param($item) $item -is [System.Management.Automation.Language.IfStatementAst] -and
  $item.Extent.Text.StartsWith('if (-not $Elevated)') }, $true) | Select-Object -First 1
if (-not $node) { throw 'Service guard was not found' }
$NoServiceRestart = $true; $Elevated = $false; $StageFleetDebug = $false; $SyncPro = $false
$stagedService = 'C:\safe\service.exe'; $builtService = 'C:\build\service.exe'; $serviceName = 'fixture'
$global:BuildCalls = 0; $global:MutationCalls = 0
function Get-ServiceImagePath { return $global:ConfiguredPath }
function Get-Service { [pscustomobject]@{ Status = 'Running' } }
function Test-ServiceProcessCurrent { return $true }
function Build-Service { $global:BuildCalls++ }
function Get-Sha256 { return 'fixture' }
function Test-Path { return $false }
function Start-ElevatedSync { $global:MutationCalls++; throw 'unexpected mutation' }
function Invoke-NonElevatedVaultGuard { $global:MutationCalls++; throw 'unexpected mutation' }
$global:ConfiguredPath = 'C:\other\service.exe'
try { & ([scriptblock]::Create($node.Extent.Text)); throw 'unstaged service was accepted' } catch {
  if ($_.Exception.Message -notmatch 'not safely staged') { throw }
}
if ($global:BuildCalls -ne 0 -or $global:MutationCalls -ne 0) { throw 'unstaged service was changed' }
$global:ConfiguredPath = $stagedService
try { & ([scriptblock]::Create($node.Extent.Text)); throw 'changed service was accepted' } catch {
  if ($_.Exception.Message -notmatch 'not current') { throw }
}
if ($global:BuildCalls -ne 1 -or $global:MutationCalls -ne 0) { throw 'changed service was restarted' }
Write-Output 'preserved'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand",
      Buffer.from(script, "utf16le").toString("base64")], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("preserved");
    expect(powershellStderrIsInformational(result.stderr)).toBe(true);
  });

  test("offers an explicit elevated launcher for privileged development checks", () => {
    expect(devLauncher).toContain("[switch]$Elevated");
    expect(devLauncher).toContain("Test-ElevatedToken");
    expect(devLauncher).toContain("Start-Process -FilePath powershell.exe -Verb RunAs");
    expect(devLauncher).toContain("-NoExit -NoProfile");
    expect(devLauncher).toContain("-File $scriptPath -Elevated");
  });

  test("runs the compiled desktop from the protected service pin in Pro development", () => {
    expect(devLauncher).toContain("'--runner', $runnerCommand");
    expect(devLauncher).not.toContain("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER");
    expect(devLauncher).toContain("--no-watch");
    expect(protectedRunnerCommand).toContain('protected-dev-runner.ps1" %*');
    expect(protectedRunner).toContain("& $cargo build -p commander-free --bin wincommander-free @buildArgs");
    expect(protectedRunner).not.toContain("& $cargo run");
    expect(protectedRunner).toContain("-SyncPro -StageFleetDebug");
    expect(protectedRunner).toContain("WinCommander\\dev-fleet\\wincommander-free.exe");
  });

  test.skipIf(process.platform !== "win32")("protected runner rejects an unexpected Tauri command before staging", () => {
    const result = spawnSync("powershell.exe", [
      "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
      "tools/protected-dev-runner.ps1", "build", "--", "unexpected.exe",
    ], { encoding: "utf8" });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("only accepts Tauri development runs");
    expect(result.stderr).not.toContain("development-service synchronization failed");
  });

  test.skipIf(process.platform !== "win32")("runner follows only a new staged app in its own session", () => {
    const script = String.raw`
Set-StrictMode -Version Latest
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
  (Join-Path (Get-Location) 'tools/protected-dev-runner.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'PowerShell parse failed' }
$definition = $ast.Find({ param($node)
  $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -eq 'Get-NewStagedProcess'
}, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:processes = @(
  [pscustomobject]@{Id=10; SessionId=2},
  [pscustomobject]@{Id=11; SessionId=3},
  [pscustomobject]@{Id=12; SessionId=2},
  [pscustomobject]@{Id=13; SessionId=2}
)
$script:images = @{10='C:\stage\wincommander-free.exe'; 11='C:\stage\wincommander-free.exe';
  12='C:\other\wincommander-free.exe'; 13='C:\stage\wincommander-free.exe'}
function Get-Process {
  [CmdletBinding()] param([string]$Name)
  return $script:processes
}
function Get-ProcessImagePath { param([int]$ProcessId) return $script:images[$ProcessId] }
$selected = Get-NewStagedProcess -Path 'C:\stage\WINCOMMANDER-FREE.EXE' -SessionId 2 -KnownIds @(10)
$script:processes = @($script:processes | Where-Object Id -ne 13)
$missing = Get-NewStagedProcess -Path 'C:\stage\wincommander-free.exe' -SessionId 2 -KnownIds @(10)
ConvertTo-Json -Compress -InputObject @($selected.Id, ($null -eq $missing))
`;
    const result = spawnSync("powershell.exe", [
      "-NoProfile", "-NonInteractive", "-EncodedCommand",
      Buffer.from(script, "utf16le").toString("base64"),
    ], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(JSON.parse(result.stdout.trim())).toEqual([13, true]);
    expect(protectedRunner).toContain("$knownIds += $active.Id");
    expect(protectedRunner).toContain("$handoffDeadline = [DateTime]::UtcNow.AddSeconds(8)");
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

  test("verifies or safely repairs the pinned driver before starting the development service", () => {
    expect(serviceSync).toContain('Repair-WcPinnedDriverAccess -Path $driverPath');
    expect(serviceSync).toContain('catch [System.UnauthorizedAccessException]');
    expect(serviceSync.lastIndexOf('Ensure-EncryptedVolumeDriver')).toBeLessThan(
      serviceSync.lastIndexOf("Start-Service -Name $serviceName"),
    );
  });
});
