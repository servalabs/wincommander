$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Evaluate only the enumeration helpers. No native erase functions run in this test.
$parsed = [Management.Automation.Language.Parser]::ParseInput($script:EraseFunctions, [ref]$null, [ref]$null)
foreach ($name in @('Get-AutoEraseFiles', 'Get-AutoEraseDirectories')) {
    $node = $parsed.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
    Invoke-Expression $node.Extent.Text
}
$script:paths = @(); $script:erased = @(); $script:denied = $false; $script:missing = $false
function Assert-AutoEraseAncestors { param($Path) }
function Get-Item {
    [CmdletBinding()] param($LiteralPath, [switch]$Force)
    if ($script:denied) { throw [UnauthorizedAccessException]::new('Synthetic access denial') }
    if ($script:missing) { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing path') }
    [pscustomobject]@{ FullName=$LiteralPath; Attributes=0 }
}
function Get-ChildItem {
    [CmdletBinding()] param($LiteralPath, $Filter, [switch]$File, [switch]$Directory, [switch]$Force, [switch]$Recurse)
    if ($Directory -and (-not $Filter -or $Filter -eq '*')) { return }
    $script:paths += @{ root=$LiteralPath; filter=$Filter; recurse=[bool]$Recurse }
    $name = if ($Directory) { $Filter.Replace('*', 'Synthetic') } else { 'synthetic.log' }
    [pscustomobject]@{ FullName=(Join-Path $LiteralPath $name); Name=$name; Attributes=0 }
}
function Erase-OneFile { param($p) $script:erased += $p }

foreach ($id in @('remoteAccessLogs','gameLauncherLogs','spotlightCache')) {
    $script:paths = @(); $script:erased = @()
    & ([scriptblock]::Create($script:AutoEraseScripts[$id]))
    $expected = switch ($id) { remoteAccessLogs { 15 }; gameLauncherLogs { 4 }; spotlightCache { 2 } }
    if ($script:paths.Count -ne $expected) { throw "Unexpected source count for $id" }
    if ($id -eq 'remoteAccessLogs' -and @($script:paths | Where-Object { $_.filter -notin @('Connections*.txt','*.trace','*.log') }).Count) { throw 'Remote log filtering widened.' }
    if ($id -eq 'gameLauncherLogs' -and @($script:paths | Where-Object { $_.root -notlike '*\Logs' }).Count) { throw 'Game logs targeted non-log state.' }
    $script:missing = $true; $script:erased = @(); $Error.Clear()
    & ([scriptblock]::Create($script:AutoEraseScripts[$id]))
    if ($script:erased.Count -or $Error.Count) { throw "Missing source was not an empty outcome for $id" }
    $script:missing = $false; $script:denied = $true; $caught = $false
    try { & ([scriptblock]::Create($script:AutoEraseScripts[$id])) } catch [UnauthorizedAccessException] { $caught = $true }
    if (-not $caught) { throw "Access denial was suppressed for $id" }
    $script:denied = $false
}
Write-Output 'Passed: 3 safe payload target sets, missing-source outcomes, and access-denied failures.'
