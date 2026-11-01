$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module could not be parsed.' }
foreach ($name in @('Get-CleanupArtifactFiles','Get-CleanupArtifactInventory','Get-CommunicationCachesInfo','Get-RemoteAccessLogsInfo','Get-GameLauncherLogsInfo','Get-EditorHistoryInfo','Get-VPNPhonebooksInfo')) {
    $def=$ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $def) { throw "Missing reader: $name" }
    . ([scriptblock]::Create($def.Extent.Text))
}
$script:case='normal'; $script:sourceIndex=0
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
    if ($LiteralPath -match '[\\/]JetBrains$') { return [pscustomobject]@{FullName=$LiteralPath} }
    $script:sourceIndex++
    if ($script:case -eq 'denied' -and $script:sourceIndex -eq 2) { throw [UnauthorizedAccessException]::new('Synthetic denied source') }
    [pscustomobject]@{FullName=$LiteralPath}
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Directory, [string]$Filter, [switch]$Force)
    if ($Directory) {
        if ($script:case -eq 'discovery-denied') { throw [UnauthorizedAccessException]::new('Synthetic IDE discovery denial') }
        return [pscustomobject]@{Name='SyntheticIDE'; FullName=(Join-Path $LiteralPath 'SyntheticIDE')}
    }
    [pscustomobject]@{FullName=(Join-Path $LiteralPath 'observation.dat'); Length=1024; LastWriteTime=[DateTime]'2026-01-01'}
    if ($script:case -eq 'late' -and $script:sourceIndex -eq 1) { throw [UnauthorizedAccessException]::new('Synthetic late denial') }
}
foreach ($reader in @('Get-CommunicationCachesInfo','Get-RemoteAccessLogsInfo','Get-GameLauncherLogsInfo','Get-EditorHistoryInfo','Get-VPNPhonebooksInfo')) {
    $script:case='normal'; $script:sourceIndex=0; $result=& $reader
    if ($result.error -or $result.partial -or $result.coverage -ne 'limited' -or $result.total -lt 2) { throw "$reader did not retain its limited complete-source inventory." }
    foreach ($case in @('denied','late')) {
        $script:case=$case; $script:sourceIndex=0; $result=& $reader
        if ($result.error -or -not $result.partial -or $result.status -ne 'partial' -or $result.total -lt 2 -or -not $result.files[0].path) { throw "$reader lost observations or failed to report $case as partial." }
    }
}
$script:case='discovery-denied'; $script:sourceIndex=0; $result=Get-EditorHistoryInfo
if (-not $result.partial -or $result.total -lt 1 -or $result.message -notmatch 'editor history') { throw 'Denied IDE discovery must preserve other editor observations and report incomplete.' }
'Passed: limited artifact inventories retain observations across source and late enumeration failures.'
