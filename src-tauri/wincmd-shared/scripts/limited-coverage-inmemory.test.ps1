$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Execute only the first statement from each real payload. The remaining
# cleanup commands are never executed by this in-memory test.
foreach ($category in @('walFiles', 'embeddedWebCache', 'recallDb', 'communicationCaches', 'editorHistory', 'remoteAccessLogs', 'gameLauncherLogs', 'vpnPhonebooks', 'geolocationCache')) {
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput($script:AutoEraseScripts[$category], [ref]$null, [ref]$errors)
    if ($errors.Count -or $ast.EndBlock.Statements.Count -eq 0) { throw "Invalid scheduled payload: $category" }
    $first = $ast.EndBlock.Statements[0].Extent.Text
    if ($first -cne '$script:AutoEraseCoverageIncomplete = $true') { throw "Missing early limited coverage flag: $category" }
    $script:AutoEraseCoverageIncomplete = $false
    . ([scriptblock]::Create($first))
    $result = New-AutoEraseResult -CompletedUtc ([DateTime]::UtcNow) -LastSuccess $null -IntervalMinutes 60 -Removed 0 -Deferred 0 -Failed 0 -CoverageIncomplete $script:AutoEraseCoverageIncomplete
    if ($result.success -or -not $result.coverageIncomplete -or $result.failed -ne 0) { throw "Misleading scheduled result: $category" }
}
$script:recallPaths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
$script:recallCalls = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
function Get-AutoEraseFiles { param($Path, $Filter, [switch]$Recurse) [void]$script:recallPaths.Add($Path); [void]$script:recallCalls.Add("$Path|$Filter|$Recurse") }
function Erase-FileSet { param($Files) }
& ([scriptblock]::Create($script:AutoEraseScripts.recallDb))
foreach ($relative in @('CoreAIPlatform.00', 'Microsoft\Windows\Recall', 'ConnectedDevicesPlatform', 'Microsoft\Windows\ActionCenter')) {
    $path = Join-Path $env:LOCALAPPDATA $relative
    if (-not $script:recallPaths.Contains($path)) { throw "Recall scheduled scope omitted $relative" }
    foreach ($pattern in @('*.db', '*.db-wal', '*.db-shm', '*.db-journal', '*.edb')) {
        if (-not $script:recallCalls.Contains("$path|$pattern|True")) { throw "Recall scheduled scope omitted $pattern in $relative" }
    }
}
if ($script:recallPaths.Count -ne 4 -or $script:recallCalls.Count -ne 20) { throw 'Recall scheduled scope contains unexpected paths or patterns.' }
Write-Output 'Passed: scoped scheduled payloads report limited coverage without reporting a deletion failure.'
