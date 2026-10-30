$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Scheduler source could not be parsed.' }
$fn = $ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-AutoEraseRecordedOutcome' }, $true)
if (-not $fn) { throw 'Outcome reader is missing.' }
. ([scriptblock]::Create($fn.Extent.Text))

$oldProgramData = $env:ProgramData; $oldLocalAppData = $env:LOCALAPPDATA
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-outcome-fixture-' + [guid]::NewGuid())
try {
    $env:ProgramData = Join-Path $tempRoot 'program'; $env:LOCALAPPDATA = Join-Path $tempRoot 'local'
    $script:SystemMaintenanceTaskPath = '\System Maintenance\'
    $scripts = Join-Path $env:ProgramData 'WinCommander\auto-erase\scripts'
    $state = Join-Path $env:LOCALAPPDATA 'WinCommander\auto-erase'
    New-Item -ItemType Directory -Path $scripts, $state -Force | Out-Null
    $payload = Join-Path $scripts 'dnsCache.user-test.ps1'
    [IO.File]::WriteAllText($payload, "`$categoryId = 'dnsCache'`n`$scopeKey = 'user-test'`n")
    $receipt = Join-Path $state 'dnsCache.user-test.result.json'
    $task = [pscustomobject]@{ TaskPath = '\System Maintenance\'; TaskName = 'SM-U-08-TEST'; Actions = @([pscustomobject]@{ Arguments = "-NoProfile -File `"$payload`"" }) }
    $identity = @{ categoryId = 'dnsCache'; ownerSid = 'S-1-5-21-1'; taskName = 'SM-U-08-TEST' }
    $result = @{ attemptedUtc = [DateTime]::UtcNow.AddSeconds(1).ToString('o'); success = $true; removed = 2; deferred = 0; failed = 0; intervalMinutes = 60 }
    [IO.File]::WriteAllText($receipt, ($result | ConvertTo-Json -Compress))
    $out = Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60
    if (-not $out -or -not $out.success -or $out.removed -ne 2) { throw 'A matching recorded outcome was not returned.' }
    $generated = Join-Path $scripts ('dnsCache.user-test.r16.g' + ('a' * 32) + '.ps1')
    [IO.File]::WriteAllText($generated, "`$categoryId = 'dnsCache'`n`$scopeKey = 'user-test'`n`$payloadGeneration = '$('a' * 32)'`n")
    $task.Actions[0].Arguments = "-NoProfile -File `"$generated`""
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A receipt from an older task generation must not be attributed to its replacement.' }
    $result.payloadGeneration = 'b' * 32
    [IO.File]::WriteAllText($receipt, ($result | ConvertTo-Json -Compress))
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A receipt from a different generation must not be attributed to this task.' }
    $result.payloadGeneration = 'a' * 32
    [IO.File]::WriteAllText($receipt, ($result | ConvertTo-Json -Compress))
    if (-not (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60)) { throw 'A generation-specific payload must retain its outcome.' }
    $task.Actions[0].Arguments = "-NoProfile -File `"$payload`""
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-2' 60) { throw 'Another user must not receive this outcome.' }
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 30) { throw 'An outcome from a different cadence must not be returned.' }
    $result.success = $false; $result.failed = 1
    [IO.File]::WriteAllText($receipt, ($result | ConvertTo-Json -Compress))
    $out = Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60
    if (-not $out -or $out.success -or $out.failed -ne 1) { throw 'An incomplete outcome was not returned.' }
    $result.success = $true
    [IO.File]::WriteAllText($receipt, ($result | ConvertTo-Json -Compress))
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A contradictory outcome must be rejected.' }
    $result.success = $false; $result.attemptedUtc = [DateTime]::UtcNow.AddDays(-1).ToString('o')
    [IO.File]::WriteAllText($receipt, ($result | ConvertTo-Json -Compress))
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A stale outcome must not be returned.' }
    $task.TaskPath = '\'
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A legacy task must not receive a canonical outcome.' }
    $systemState = Join-Path $env:ProgramData 'WinCommander\auto-erase\state'
    New-Item -ItemType Directory -Path $systemState -Force | Out-Null
    $systemPayload = Join-Path $scripts 'dnsCache.system.ps1'
    [IO.File]::WriteAllText($systemPayload, "`$categoryId = 'dnsCache'`n`$scopeKey = 'system'`n")
    $systemReceipt = Join-Path $systemState 'dnsCache.system.result.json'
    $result.attemptedUtc = [DateTime]::UtcNow.AddSeconds(1).ToString('o')
    [IO.File]::WriteAllText($systemReceipt, ($result | ConvertTo-Json -Compress))
    $task.TaskPath = '\System Maintenance\'; $task.Actions[0].Arguments = "-File `"$systemPayload`""
    $identity.ownerSid = 'S-1-5-18'
    if (-not (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60)) { throw 'A matching system outcome was not returned.' }
    $identity.ownerSid = 'S-1-5-21-1'
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A user task must not receive a system outcome.' }
    Write-Output 'Passed: outcome binding, account scope, cadence, stale and incomplete receipts.'
} finally {
    $env:ProgramData = $oldProgramData; $env:LOCALAPPDATA = $oldLocalAppData
    if ((Split-Path -Parent $tempRoot) -eq [IO.Path]::GetTempPath().TrimEnd('\') -and (Split-Path -Leaf $tempRoot) -like 'wc-outcome-fixture-*') {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
