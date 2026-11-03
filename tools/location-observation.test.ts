import { expect, test } from "bun:test";

const powershell = process.platform === "win32" ? "powershell.exe" : "pwsh";

test("location actions propagate failed observations without calling Windows", async () => {
  const script = String.raw`
$ProgressPreference = 'SilentlyContinue'
$path = Join-Path (Get-Location) 'src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
foreach ($name in @('Disable-LocationTracking', 'Enable-LocationTracking')) {
  $nodes = $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
  if ($nodes.Count -ne 1) { throw 'Unexpected action definition count' }
  Invoke-Expression $nodes[0].Extent.Text
}
function Assert-IsAdmin {}
function Test-Path { return $true }
function Set-ItemProperty {}
function Remove-ItemProperty {}
function Set-AppCapabilityAccess {}
function Set-LocationServiceMaster {}
function Get-LocationTrackingStatus { return $script:observation }
foreach ($name in @('Disable-LocationTracking', 'Enable-LocationTracking')) {
  foreach ($observation in @(@{error=$true;disabled=$false}, @{disabled=$null}, @{disabled='false'})) {
    $script:observation = $observation
    $actual = & $name
    if ($actual.error -ne $true -or $actual.ContainsKey('disabled')) { throw "$name accepted an invalid observation" }
  }
  foreach ($value in @($true, $false)) {
    $script:observation = @{disabled=$value}
    $actual = & $name
    $expected = $name -eq 'Disable-LocationTracking'
    if ($value -eq $expected) {
      if ($actual.error -eq $true -or $actual.disabled -ne $value) { throw "$name rejected a matching observation" }
    } elseif ($actual.error -ne $true -or $actual.ContainsKey('disabled')) {
      throw "$name accepted a contradictory observation"
    }
  }
}
`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
  const process = Bun.spawn([powershell, "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
}, 15_000);

test("location observation distinguishes absent policy from failed reads without touching Windows", async () => {
  const script = String.raw`
$ProgressPreference = 'SilentlyContinue'
$path = Join-Path (Get-Location) 'src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
$fn=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-LocationTrackingStatus'},$true)
Invoke-Expression $fn.Extent.Text
function Read-LocationRegistryValue { param($Hive,$Path,$Name)
  if ($script:failure -and $Path -like $script:failure) { throw 'fixture read denied' }
  if ($Path -like '*lfsvc*') { return @{ present=(-not $script:missingMaster); value=$script:master } }
  if ($Name -eq 'SensorPermissionState' -and $null -ne $script:sensor) { return @{ present=$true; value=$script:sensor } }
  if ($script:blocked -and $Path -like '*LocationAndSensors*') { return @{ present=$true; value=1 } }
  return @{ present=$false; value=$null }
}
function Get-Service { param($Name,$ErrorAction) if ($script:serviceFailure) { throw 'fixture service denied' }; return @{ StartType=$script:serviceStart } }
$script:sensor=$null; $script:master=1; $script:failure=$null; $script:blocked=$false; $script:serviceFailure=$false; $script:missingMaster=$false; $script:serviceStart='Manual'
$r=Get-LocationTrackingStatus; if ($r.error -or $r.disabled -ne $false) { throw 'optional absent policy should permit known state' }
$script:master=0; $r=Get-LocationTrackingStatus; if ($r.error -or $r.disabled -ne $true) { throw 'master off was missed' }
$script:master=1; $script:sensor=0; $r=Get-LocationTrackingStatus; if ($r.error -or $r.disabled -ne $true) { throw 'sensor deny was missed' }
$script:sensor=$null; $script:blocked=$true; $r=Get-LocationTrackingStatus; if ($r.error -or $r.disabled -ne $true) { throw 'policy deny was missed' }
$script:failure='*AppPrivacy*'; $r=Get-LocationTrackingStatus; if ($r.error -ne $true -or $null -ne $r.disabled) { throw 'denied read became known' }
$script:failure=$null; $script:serviceFailure=$true; $r=Get-LocationTrackingStatus; if ($r.error -ne $true -or $null -ne $r.disabled) { throw 'failed service became known' }
$script:serviceFailure=$false; $script:missingMaster=$true; $r=Get-LocationTrackingStatus; if ($r.error -ne $true -or $null -ne $r.disabled) { throw 'missing master became known' }
$script:missingMaster=$false; $script:serviceStart='Disabled'; $r=Get-LocationTrackingStatus; if ($r.error -or $r.disabled -ne $true) { throw 'disabled service was missed' }
$script:serviceStart='Manual'; $script:master='invalid'; $r=Get-LocationTrackingStatus; if ($r.error -ne $true -or $null -ne $r.disabled) { throw 'malformed master became known' }
`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
  const process = Bun.spawn([powershell, "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
}, 15_000);


test("startup and hardening preserve unknown location observations", async () => {
  const bridge = await Bun.file("src-tauri/commander-free/scripts/core/settings-bridge.ps1").text();
  const hardening = await Bun.file("src-tauri/commander-free/scripts/modules/tweaks/system.ps1").text();
  const backend = await Bun.file("src-tauri/commander-free/src/backend.rs").text();
  expect(bridge).toContain("locationTrackingDisabled   = $null");
  expect(bridge).toContain("$locationStatus = Get-LocationTrackingStatus");
  expect(bridge).toContain("$locationStatus.disabled -is [bool]");
  expect(hardening).toContain("$locationStatus.disabled -isnot [bool]) { $null }");
  const composition = backend.slice(backend.indexOf('} else if command == "Get-WCSystemProbe"'), backend.indexOf('} else if command == "Get-HardeningStatus"'));
  expect(composition).toContain('command == "Get-DashboardPrivacyPolicyStatus"');
  expect(composition).toContain('load_module("privacy/telemetry")');
});

test("actual location probe branches preserve known, unknown, and migration semantics", async () => {
  const script = String.raw`
$ProgressPreference='SilentlyContinue'
$tokens=$null; $errors=$null
$bridge=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/scripts/core/settings-bridge.ps1'),[ref]$tokens,[ref]$errors)
$probe=$bridge.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-WCSystemProbe'},$true)
$check=$probe.Body.Find({param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $n.Extent.Text -match 'Get-LocationTrackingStatus'},$true)
if (-not $check) { throw 'probe branch missing' }
$branch=[scriptblock]::Create($check.Extent.Text)
function Get-LocationTrackingStatus { return $script:observation }
foreach ($value in @($true,$false)) {
  $state=@{privacy=@{telemetry=@{locationTrackingDisabled=$null}}}; $script:observation=@{disabled=$value}; . $branch
  if ($state.privacy.telemetry.locationTrackingDisabled -ne $value) { throw 'known probe state changed' }
}
foreach ($value in @(@{error=$true;disabled=$null},@{disabled='bad'})) {
  $state=@{privacy=@{telemetry=@{locationTrackingDisabled=$null}}}; $script:observation=$value; . $branch
  if ($null -ne $state.privacy.telemetry.locationTrackingDisabled) { throw 'unknown probe became known' }
}
$migration=$bridge.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-WCMigrationData'},$true)
$legacy=$migration.Body.Find({param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $n.Extent.Text -match 'locCam'},$true)
if (-not $legacy -or $legacy.Extent.Text -match 'Get-LocationTrackingStatus') { throw 'legacy migration changed' }
function Get-ItemProperty { param($Path,$Name,$EA) if ($script:blocked -and $Name -eq 'Value') { return @{Value='Deny'} }; return $null }
foreach ($blocked in @($true,$false)) {
  $script:blocked=$blocked; $migration=@{privacy=@{telemetry=@{}}}; . ([scriptblock]::Create($legacy.Extent.Text))
  if ($blocked -and $migration.privacy.telemetry.locationTrackingDisabled -ne $true) { throw 'migration lost disabled setting' }
  if (-not $blocked -and $migration.privacy.telemetry.ContainsKey('locationTrackingDisabled')) { throw 'migration invented enabled setting' }
}
$system=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/scripts/modules/tweaks/system.ps1'),[ref]$tokens,[ref]$errors)
$hardening=$system.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-HardeningStatus'},$true)
$maps=@($hardening.Body.FindAll({param($n) $n -is [Management.Automation.Language.HashtableAst]},$true))
$entry=@($maps | ForEach-Object { $_.KeyValuePairs } | Where-Object { $_.Item1.Extent.Text -eq 'locationTrackingDisabled' })
if ($entry.Count -ne 1) { throw 'hardening field missing' }
$expr=[scriptblock]::Create($entry[0].Item2.Extent.Text)
foreach ($value in @(@{error=$true;disabled=$null},@{disabled='bad'})) { $locationStatus=$value; if ($null -ne (. $expr)) { throw 'unknown hardening state became known' } }
foreach ($value in @($true,$false)) { $locationStatus=@{disabled=$value}; if ((. $expr) -ne $value) { throw 'known hardening state changed' } }
`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
  const process = Bun.spawn([powershell, "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
}, 15_000);
