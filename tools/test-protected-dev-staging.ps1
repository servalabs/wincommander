[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$scriptPath = Join-Path $PSScriptRoot 'sync-dev-service.ps1'
$parseErrors = $null
$tree = [Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'The development service script does not parse.' }
foreach ($name in @('Assert-NoReparsePoint', 'Assert-ProtectedPath', 'Invoke-SafeServiceRecovery', 'Get-PriorRecoveryPlan', 'Test-SimpleServiceImagePath')) {
    $definition = $tree.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing staging validator: $name" }
    Invoke-Expression $definition.Extent.Text
}
$knownImage = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
if (-not (Test-SimpleServiceImagePath $knownImage)) { throw 'An existing simple executable path was rejected.' }
foreach ($badPath in @("$knownImage --flag", '"C:\example.exe" --flag', '%SystemRoot%\system32\example.exe', 'relative.exe')) {
    if (Test-SimpleServiceImagePath $badPath) { throw 'A non-simple prior service command was accepted.' }
}
if ((Get-PriorRecoveryPlan $true 'Stopped' $false $false $true) -ne 'config-only') { throw 'A previously stopped service must retain its prior configuration without starting.' }
if ((Get-PriorRecoveryPlan $true 'Running' $false $false $true) -ne 'abort') { throw 'An unprotected running service must not be stopped.' }
if ((Get-PriorRecoveryPlan $true 'Stopped' $false $true $true) -ne 'abort') { throw 'A prior image that staging would overwrite needs a verified backup.' }
if ((Get-PriorRecoveryPlan $true 'Running' $true $true $true) -ne 'full') { throw 'A protected running service backup should permit recovery.' }
$script:recoveryCalls = 0
$script:restartCalls = 0
$restore = { param([bool]$restart) $script:recoveryCalls++; if ($restart) { $script:restartCalls++ } }
if (-not (Invoke-SafeServiceRecovery $true $true ('a' * 64) 'Stopped' $restore) -or $script:restartCalls -ne 1) { throw 'A previously running service was not restarted.' }
if (-not (Invoke-SafeServiceRecovery $true $false ('a' * 64) 'Stopped' $restore) -or $script:restartCalls -ne 1) { throw 'An intentionally stopped service was restarted.' }
foreach ($case in @(@($true, $true, '', 'Stopped'), @($true, $true, ('a' * 64), 'Running'), @($false, $true, ('a' * 64), 'Stopped'), @($true, $true, ('a' * 64), 'Missing'))) {
    if (Invoke-SafeServiceRecovery $case[0] $case[1] $case[2] $case[3] $restore) { throw 'Recovery ran without a safely stopped prior service and verified backup.' }
}
if ($script:recoveryCalls -ne 2) { throw 'Unsafe recovery unexpectedly attempted a service action.' }
$failed = $false
try { Invoke-SafeServiceRecovery $true $true ('a' * 64) 'Stopped' { throw 'recovery failed' } } catch { $failed = $true }
if (-not $failed) { throw 'A recovery failure was hidden.' }
$parent = Join-Path $env:ProgramData 'WinCommander'
if (Test-Path -LiteralPath $parent) { Assert-ProtectedPath $parent }
$temporaryFile = [IO.Path]::GetTempFileName()
try {
    $denied = $false
    try { Assert-ProtectedPath $temporaryFile } catch { $denied = $true }
    if (-not $denied) { throw 'A user-owned temporary file was accepted as protected.' }
} finally {
    Remove-Item -LiteralPath $temporaryFile -Force
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('wincommander-staging-test-' + [guid]::NewGuid().ToString())
$target = Join-Path $root 'target'
$junction = Join-Path $root 'junction'
try {
    New-Item -ItemType Directory -Path $target -Force | Out-Null
    New-Item -ItemType Junction -Path $junction -Target $target | Out-Null
    $denied = $false
    try { Assert-NoReparsePoint $junction } catch { $denied = $true }
    if (-not $denied) { throw 'A junction was accepted as a protected development path.' }
} finally {
    if (Test-Path -LiteralPath $junction) { Remove-Item -LiteralPath $junction -Force }
    if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Force }
}
Write-Host 'Protected staging validation passed.'
