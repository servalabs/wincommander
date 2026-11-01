$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupProcessElevation', 'Get-ProcessIntelligence')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}

# Read this existing process only; compare the token result with Windows' own
# enabled Administrators check. No process or account is modified.
$ownElevation = Get-CleanupProcessElevation -ProcessId $PID
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
$expected = if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { 'Yes' } else { 'No' }
if ($ownElevation -ne $expected) { throw 'Current process token elevation did not match the Windows identity.' }
if ((Get-CleanupProcessElevation -ProcessId -1) -ne 'Unknown') { throw 'An inaccessible process must have unknown elevation.' }

# Validate output mapping and ensure inaccessible tokens are not called No.
function Get-CleanupProcessElevation { param([int]$ProcessId) switch ($ProcessId) { 1 { 'Yes' } 2 { 'No' } default { 'Unknown' } } }
function Get-Process { param($ErrorAction) @([pscustomobject]@{ Id=1; ProcessName='one'; MainModule=$null }, [pscustomobject]@{ Id=2; ProcessName='two'; MainModule=$null }, [pscustomobject]@{ Id=3; ProcessName='three'; MainModule=$null }) }
$result = Get-ProcessIntelligence
if ($result.error -or @($result.processes).Count -ne 3 -or $result.processes[0].elevated -ne 'Yes' -or $result.processes[1].elevated -ne 'No' -or $result.processes[2].elevated -ne 'Unknown') { throw 'Process elevation results were not preserved accurately.' }
$exited = [pscustomobject]@{ Id=4; MainModule=$null }
$exited | Add-Member -MemberType ScriptProperty -Name ProcessName -Value { throw 'The process exited.' }
function Get-Process { param($ErrorAction) @([pscustomobject]@{ Id=1; ProcessName='one'; MainModule=$null }, $exited, [pscustomobject]@{ Id=2; ProcessName='two'; MainModule=$null }) }
$result = Get-ProcessIntelligence
if ($result.error -or -not $result.partial -or @($result.processes).Count -ne 2 -or $result.processes[0].name -ne 'one' -or $result.processes[1].name -ne 'two') { throw 'An exited process must not discard other process observations.' }
Write-Output 'Passed: process elevation uses token readback and preserves inaccessible results as unknown.'
