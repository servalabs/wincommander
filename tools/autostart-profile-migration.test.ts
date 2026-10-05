import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

test.skipIf(process.platform !== "win32")("runtime migration reads raw Run values and uses each loaded hive's profile", () => {
  const source = readFileSync("src-tauri/commander-free/src/autostart.rs", "utf8");
  const match = source.match(/const POWERSHELL_COMMON: &str = r#"([\s\S]*?)"#;/);
  if (!match) throw new Error("Missing runtime migration script");
  const common = match[1].replace(/__[A-Z_]+__/g, token => ({
    __TARGET_EXE__: "(Join-Path $env:ProgramFiles 'WinCommander\\wincommander-free.exe')",
    __DESIRED_TASK_NAME__: "'SM-AS'", __ALTERNATE_TASK_NAME__: "'SM-AS'",
    __TASK_PATH__: "'\\System Maintenance\\'", __PREFERENCE_PATH__: "'unused'",
    __PREFERENCE_VALUE_NAME__: "'AutostartEnabled'", __LEGACY_DATA_DIR_NAME__: "'WinCommander'",
    __RUN_VALUE_NAMES__: "'WinCommander', 'WinCommander Pro'",
  })[token] ?? token);
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", "$code=[Console]::In.ReadToEnd(); & ([scriptblock]::Create($code))"], {
    encoding: "utf8", windowsHide: true,
    input: `${common}
$fixture='HKCU:\\Software\\ServaLabs\\WinCommander\\InstallerTests\\'+[guid]::NewGuid().ToString('N')
$otherProfile=Join-Path $env:SystemDrive 'Users\\StartupMigrationFixture'
try {
  New-Item -Path $fixture -Force | Out-Null
  $command='"%LOCALAPPDATA%\\Programs\\WinCommander\\wincommander-free.exe" --minimized'
  New-ItemProperty -LiteralPath $fixture -Name 'WinCommander Pro' -PropertyType ExpandString -Value $command | Out-Null
  New-ItemProperty -LiteralPath $fixture -Name 'WinCommander Free' -PropertyType String -Value '"C:\\Other\\wincommander-free.exe" --autostart' | Out-Null
  if ((Get-RegistryValueOrNull $fixture 'WinCommander Pro') -cne $command) { throw 'REG_EXPAND_SZ was expanded under the administrator instead of its owner' }
  $resolved='"'+(Join-Path $otherProfile 'AppData\\Local\\Programs\\WinCommander\\wincommander-free.exe')+'" --minimized'
  if (-not (Test-OwnedExecutableCommand $resolved $otherProfile)) { throw 'Other profile absolute route was missed' }
  if (-not (Test-OwnedExecutableCommand $command $otherProfile)) { throw 'Other profile environment route was missed' }
  if (Test-OwnedExecutableCommand $command $null) { throw 'Unknown profile inherited administrator environment' }
  if (Test-OwnedExecutableCommand ($resolved.Replace('.exe','.exe.other')) $otherProfile) { throw 'Lookalike executable accepted' }
  if (Test-OwnedExecutableCommand '"%LOCALAPPDATA%\\Foreign\\wincommander-free.exe" --autostart' $otherProfile) { throw 'Foreign per-user executable accepted' }
  if ((Get-RunOwnerProfile 'Registry::HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run') -ne $env:USERPROFILE) { throw 'Current profile lookup failed' }
  function Test-IsAdministrator { return $true }
  function Get-ChildItem { [pscustomobject]@{PSChildName='S-1-5-21-11-22-33-1001'}; [pscustomobject]@{PSChildName='S-1-5-21-11-22-33-1001_Classes'} }
  $loaded=@(Get-RunPaths | Where-Object { $_ -like '*HKEY_USERS*' })
  if ($loaded.Count -ne 4 -or ($loaded -match '_Classes').Count -ne 0) { throw 'Loaded account Run/RunOnce routes were not discovered correctly' }
  $originalRead=(Get-Command Get-RegistryValueOrNull).ScriptBlock
  function Get-RegistryValueOrNull($Path,$Name) {
    if ($Name -eq 'ProfileImagePath' -and $Path.EndsWith('S-1-5-21-11-22-33-1001')) { return $otherProfile }
    & $originalRead $Path $Name
  }
  if ((Get-RunOwnerProfile $loaded[0]) -ne $otherProfile) { throw 'Loaded hive owner lookup failed' }
  function Get-RunOwnerProfile { return $otherProfile }
  Remove-OwnedRunValues -Paths @($fixture)
  if ($null -ne (Get-RegistryValueOrNull $fixture 'WinCommander Pro')) { throw 'Owned Pro-labelled route survived migration' }
  if ($null -eq (Get-RegistryValueOrNull $fixture 'WinCommander Free')) { throw 'Foreign route was removed' }
  Remove-OwnedRunValues -Paths @($fixture)
  'PASS raw registry values, loaded hive ownership, exact paths, cleanup, and idempotence'
} finally {
  if ((Split-Path $fixture) -eq 'HKCU:\\Software\\ServaLabs\\WinCommander\\InstallerTests' -and (Split-Path $fixture -Leaf) -match '^[a-f0-9]{32}$') { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
`,
  });
  expect(result.stderr.trim()).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("PASS raw registry values, loaded hive ownership, exact paths, cleanup, and idempotence");
});

test("runtime migration recognizes the historical Pro-labelled Run value", () => {
  const source = readFileSync("src-tauri/commander-free/src/autostart.rs", "utf8");
  expect(source).toContain('"WinCommander Pro".to_string()');
  expect(source).toContain("return $arguments -in @('', '--autostart', '--minimized')");
});
