import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";

describe("installer task readback", () => {
  test.skipIf(process.platform !== "win32")("normalizes group names and rejects the wrong privilege, executable, arguments and session policy", () => {
    const script = `
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'),[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Invalid installer script' }
$function=$ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-TaskContract' },$true)
Invoke-Expression $function.Extent.Text
$targetPath='C:\\Program Files\\WinCommander\\wincommander-free.exe'
$group=([Security.Principal.SecurityIdentifier]'S-1-5-32-544').Translate([Security.Principal.NTAccount]).Value
function Fixture {
  [pscustomobject]@{Principal=[pscustomobject]@{GroupId=$group;RunLevel='Highest'};Settings=[pscustomobject]@{MultipleInstances='Parallel'};Actions=@([pscustomobject]@{Execute=$targetPath;Arguments='--elevated-relaunch'})}
}
Assert-TaskContract (Fixture) 'S-1-5-32-544' 'Highest' '--elevated-relaunch'
$sidFixture=Fixture; $sidFixture.Principal.GroupId='S-1-5-32-544'
Assert-TaskContract $sidFixture 'S-1-5-32-544' 'Highest' '--elevated-relaunch'
foreach ($case in @('group','level','instances','path','arguments')) {
  $t=Fixture
  switch ($case) {
    group { $t.Principal.GroupId='S-1-5-32-545' }
    level { $t.Principal.RunLevel='Limited' }
    instances { $t.Settings.MultipleInstances='IgnoreNew' }
    path { $t.Actions[0].Execute='C:\\wrong.exe' }
    arguments { $t.Actions[0].Arguments='--wrong' }
  }
  $denied=$false
  try { Assert-TaskContract $t 'S-1-5-32-544' 'Highest' '--elevated-relaunch' } catch { $denied=$true }
  if (-not $denied) { throw "Invalid task accepted: $case" }
}
Write-Output 'PASS'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe("PASS");
  });
});
