import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

test("schedule readback preserves old and current bulk ownership without changing tasks", () => {
  const script = String.raw`
$ProgressPreference = 'SilentlyContinue'
$path = Join-Path (Get-Location) 'src-tauri/wincmd-shared/scripts/auto-erase.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
foreach ($name in @('Test-AutoEraseManagedTask', 'Test-AutoEraseTaskEnabled', 'Get-AutoEraseSchedules')) {
  $fn = $ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}, $true)
  Invoke-Expression $fn.Extent.Text
}
function Get-ScheduledTask { $script:tasks }
function Get-AutoEraseTaskIdentity { param($task) @{ categoryId = 'dnsCache'; ownerSid = 'S-1-5-18' } }
function Get-ScheduledTaskInfo { $null }
function Get-AutoEraseRecordedOutcome { $null }
$script:tasks = @(
  [pscustomobject]@{ TaskPath='\'; TaskName='old'; Description='WinCommander Auto-set scheduled wipe v2'; State='Ready'; Principal=@{UserId='SYSTEM'}; Triggers=@() },
  [pscustomobject]@{ TaskPath='\'; TaskName='new'; Description='System Maintenance scheduled cleanup v4 (managed)'; State='Ready'; Principal=@{UserId='SYSTEM'}; Triggers=@() },
  [pscustomobject]@{ TaskPath='\'; TaskName='current'; Description='System Maintenance scheduled cleanup v22 (managed)'; State='Ready'; Principal=@{UserId='SYSTEM'}; Triggers=@() },
  [pscustomobject]@{ TaskPath='\'; TaskName='manual'; Description='System Maintenance scheduled cleanup v4'; State='Ready'; Principal=@{UserId='SYSTEM'}; Triggers=@() }
)
$result = Get-AutoEraseSchedules
if ($result.error -or $result.schedules.Count -ne 4) { throw 'schedule readback failed' }
if (-not $result.schedules[0].managedByAutoSet -or -not $result.schedules[1].managedByAutoSet -or -not $result.schedules[2].managedByAutoSet -or $result.schedules[3].managedByAutoSet) { throw 'bulk ownership was misclassified' }
`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(script, "utf16le").toString("base64")]);
  expect({ code: result.status, stderr: result.stderr.toString() }).toEqual({ code: 0, stderr: "" });
});


test("migration preserves the same ownership classification", () => {
  const source = readFileSync("src-tauri/wincmd-shared/scripts/auto-erase.ps1", "utf8");
  const migration = source.slice(source.indexOf("function Invoke-AutoEraseMigration"));
  expect(migration).toContain("elseif (Test-AutoEraseManagedTask $task)");
});
